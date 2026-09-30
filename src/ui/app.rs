//! 应用状态层：与 Win32 细节解耦，只操作引擎 Doc 并记录 UI 映射

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::os::windows::fs::MetadataExt;

use windows::Win32::Foundation::{FILETIME, HWND, SYSTEMTIME};
use windows::Win32::Graphics::Gdi::HFONT;
use windows::Win32::Storage::FileSystem::FileTimeToLocalFileTime;
use windows::Win32::System::Time::FileTimeToSystemTime;

use hostsmate::engine::conflict::{ConflictGroup, ConflictKind};
use hostsmate::engine::{self, Doc, Entry, Record, Scheme, DEFAULT_SCHEME};

// 工具栏命令 ID
pub const IDM_NEW: i32 = 1001;
pub const IDM_DELETE: i32 = 1002;
pub const IDM_SAVE: i32 = 1003;
pub const IDM_IMPORT: i32 = 1005;
pub const IDM_EXPORT: i32 = 1006;
pub const IDM_SETTINGS: i32 = 1007;
pub const IDM_ABOUT: i32 = 1008;
pub const IDM_FOCUS_SEARCH: i32 = 2101;

// 子控件 ID（WM_COMMAND 路由）
pub const IDC_SEARCH: i32 = 10;

// 右键菜单命令 ID
pub const CM_CLEANUP: i32 = 2011;
pub const CM_TOGGLE: i32 = 2001;
pub const CM_EDIT: i32 = 2002;
pub const CM_DELETE: i32 = 2003;
pub const CM_PIN: i32 = 2004;
pub const CM_COPY: i32 = 2005;
pub const CM_PING: i32 = 2006;
pub const CM_PINGT: i32 = 2007;
pub const CM_PING6: i32 = 2008;
pub const CM_PINGIP: i32 = 2009;
pub const CM_OPENHTTP: i32 = 2010;

// 设置窗控件 ID
pub const IDOK: i32 = 1;
pub const IDCANCEL: i32 = 2;
pub const IDC_DNS: i32 = 3001;
pub const IDC_KEEP: i32 = 3002;
pub const IDC_ADMIN: i32 = 3003;
pub const IDC_BKLIST: i32 = 3004;
pub const IDC_RESTORE: i32 = 3005;
pub const IDC_OPENBK: i32 = 3006;
pub const IDC_CAT_RESET: i32 = 3011;
pub const IDC_CATLIST: i32 = 3012;
pub const IDC_CATNAME: i32 = 3013;
pub const IDC_CATKW: i32 = 3014;
pub const IDC_CAT_ADD: i32 = 3015;
pub const IDC_CAT_DEL: i32 = 3016;
pub const IDC_CAT_UP: i32 = 3017;
pub const IDC_CAT_DOWN: i32 = 3018;
pub const IDC_BKDEL: i32 = 3019;

// 右表列
pub const COL_SELECT: usize = 0;
pub const COL_DOMAIN: usize = 1;
pub const COL_IP: usize = 2;
pub const COL_COMMENT: usize = 3;
pub const COL_ENABLED: usize = 4;
pub const COL_CONFLICT: usize = 5;
pub const COL_ACTIONS: usize = 6;

// 状态筛选 chips / 排序
pub const CHIP_ALL: i32 = 2201;
pub const CHIP_ENABLED: i32 = 2202;
pub const CHIP_DISABLED: i32 = 2203;
pub const CHIP_CONFLICT: i32 = 2204;
pub const IDM_SORT: i32 = 2205;
pub const IDM_SORT_FILE: i32 = 2301;
pub const IDM_SORT_DOMAIN: i32 = 2302;
pub const IDM_SORT_IP: i32 = 2303;
pub const IDM_STATUS: i32 = 2206;
pub const CAT_ALL: i32 = 2401;
/// 兜底分类「自定义」chip（全不命中时归入）
pub const CAT_FALLBACK: i32 = 2406;
/// 自定义分类 chip 基址：第 i 个分类 → CAT_BASE + i
pub const CAT_BASE: i32 = 2410;
pub const IDC_RECORDS: i32 = 13;

/// chip ID 是否为分类筛选（全部 / 自定义兜底 / 任一自定义分类）
pub fn is_category_chip(id: i32) -> bool {
    id == CAT_ALL || id == CAT_FALLBACK || (id >= CAT_BASE && id < CAT_BASE + 64)
}

/// 分类 chip 的基准宽度（96 DPI 设计值；窄窗口下按比例压缩）
pub fn chip_base_w(id: i32) -> i32 {
    if id == CAT_ALL {
        112
    } else if id == CAT_FALLBACK {
        136
    } else {
        121
    }
}

/// 状态筛选（chips）
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FilterState {
    All,
    Enabled,
    Disabled,
    Conflicted,
}

/// 分类筛选：All = 全部；Cat(i) = 第 i 个自定义分类，i == 分类数 = 兜底「自定义」
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CatFilter {
    All,
    Cat(usize),
}

/// 一个用户自定义分类：名称 + 关键词
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserCategory {
    pub name: String,
    pub keywords: Vec<String>,
}

/// 分类规则：列表顺序即优先级，自上而下逐类尝试，全不命中归兜底「自定义」。
/// 关键词默认对域名和备注做「包含」匹配；以 * 结尾的关键词只按域名前缀匹配
/// （如 ad.* 命中 ad.example.com，但不会误伤 download.com）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategoryRules {
    pub cats: Vec<UserCategory>,
}

impl Default for CategoryRules {
    fn default() -> Self {
        Self {
            cats: vec![
                UserCategory { name: "CDN".into(), keywords: vec!["cdn".into(), "静态资源".into()] },
                UserCategory {
                    name: "分析".into(),
                    keywords: vec!["分析".into(), "统计".into(), "analysis".into(), "advstat".into(), "biz".into()],
                },
                UserCategory {
                    name: "跟踪".into(),
                    keywords: vec!["跟踪".into(), "追踪".into(), "埋点".into(), "track".into()],
                },
                UserCategory {
                    name: "广告".into(),
                    keywords: vec!["广告".into(), "ad.*".into(), "adsp.*".into(), "advert".into(), "xunlei".into()],
                },
            ],
        }
    }
}

impl CategoryRules {
    /// 关键词列表文本 → Vec：按中英文逗号/顿号/分号分隔，去空项
    pub fn parse_list(text: &str) -> Vec<String> {
        text.split([',', '，', '、', ';', '；'])
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect()
    }

    /// Vec → 关键词列表文本（设置窗回显用）
    pub fn join_list(list: &[String]) -> String {
        list.join(",")
    }
}

fn kw_hit(kw: &str, domain: &str, note: &str) -> bool {
    let kw = kw.trim().to_lowercase();
    if kw.is_empty() {
        return false;
    }
    match kw.strip_suffix('*') {
        Some(prefix) => domain.starts_with(prefix),
        None => domain.contains(&kw) || note.contains(&kw),
    }
}

impl CategoryRules {
    /// 命中返回分类下标；全不命中返回 cats.len()（兜底「自定义」）
    pub fn classify(&self, r: &Record) -> usize {
        let domain = r.domain.to_lowercase();
        let note = r.comment.as_deref().unwrap_or("").to_lowercase();
        for (i, cat) in self.cats.iter().enumerate() {
            if cat.keywords.iter().any(|kw| kw_hit(kw, &domain, &note)) {
                return i;
            }
        }
        self.cats.len()
    }
}

/// 排序方式（仅影响显示，不影响文件顺序）
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    FileOrder,
    Domain,
    Ip,
}

/// 冲突行角色（规格 §2.3）
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConflictRole {
    /// 真冲突中的生效行
    RealEffective,
    /// 真冲突中被覆盖行
    RealOverridden,
    /// 冗余重复中的保留行
    RedundantKept,
    /// 冗余重复中的多余行
    RedundantExtra,
}

pub fn role_text(role: ConflictRole) -> &'static str {
    match role {
        ConflictRole::RealEffective | ConflictRole::RedundantKept => "✓ 生效",
        ConflictRole::RealOverridden => "✗ 被覆盖",
        ConflictRole::RedundantExtra => "－ 冗余",
    }
}

/// 应用设置（settings.ini；分类规则一并存于此文件）
#[derive(Clone, Debug)]
pub struct Settings {
    pub flushdns: bool,
    pub backup_keep: u32,
    pub rules: CategoryRules,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            flushdns: true,
            backup_keep: 20,
            rules: CategoryRules::default(),
        }
    }
}

impl Settings {
    fn path(dir: &Path) -> PathBuf {
        dir.join("settings.ini")
    }

    pub fn load(dir: &Path) -> Self {
        let mut s = Self::default();
        let Ok(text) = std::fs::read_to_string(Self::path(dir)) else {
            return s;
        };
        // 新格式：cat_names=名称,名称… + cat_<i>=关键词；缺省行 = 该分类无关键词。
        // 旧格式（cat_cdn 等四键）：键缺省 = 默认关键词，存在但为空 = 用户清空
        let mut names: Option<String> = None;
        let mut kws: Vec<Option<String>> = Vec::new();
        let mut legacy: [Option<String>; 4] = Default::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            match k.trim() {
                "flushdns" => s.flushdns = v.trim() == "1",
                "backup_keep" => {
                    s.backup_keep = v.trim().parse().unwrap_or(20).clamp(1, 200);
                }
                "cat_names" => names = Some(v.to_string()),
                _ => {
                    let key = k.trim();
                    let idx = key.strip_prefix("cat_").and_then(|n| n.parse::<usize>().ok());
                    if let Some(i) = idx {
                        if kws.len() <= i {
                            kws.resize(i + 1, None);
                        }
                        kws[i] = Some(v.to_string());
                    } else {
                        match key {
                            "cat_cdn" => legacy[0] = Some(v.to_string()),
                            "cat_analysis" => legacy[1] = Some(v.to_string()),
                            "cat_tracking" => legacy[2] = Some(v.to_string()),
                            "cat_ads" => legacy[3] = Some(v.to_string()),
                            _ => {}
                        }
                    }
                }
            }
        }
        if let Some(names_text) = names {
            s.rules.cats.clear(); // 覆盖默认四类，以 ini 为准
            for (i, n) in CategoryRules::parse_list(&names_text).into_iter().enumerate() {
                let keywords = kws.get(i).cloned().flatten()
                    .map_or_else(Vec::new, |v| CategoryRules::parse_list(&v));
                // 名称为空时给占位名，避免 chip 上出现空白
                let name = if n.trim().is_empty() { format!("分类{}", i + 1) } else { n };
                s.rules.cats.push(UserCategory { name, keywords });
            }
        } else if legacy.iter().any(|x| x.is_some()) {
            // 旧版 settings.ini：按默认四类名迁移，未写的键沿用默认关键词
            let d = CategoryRules::default();
            let keys = [legacy[0].as_deref(), legacy[1].as_deref(), legacy[2].as_deref(), legacy[3].as_deref()];
            s.rules.cats = d.cats.iter().zip(keys).map(|(c, k)| UserCategory {
                name: c.name.clone(),
                keywords: k.map_or_else(|| c.keywords.clone(), |v| CategoryRules::parse_list(v)),
            }).collect();
        }
        s
    }

    pub fn store(&self, dir: &Path) {
        let mut text = format!("flushdns={}\nbackup_keep={}\ncat_names={}\n",
            self.flushdns as u32,
            self.backup_keep,
            self.rules.cats.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(","),
        );
        for (i, c) in self.rules.cats.iter().enumerate() {
            text.push_str(&format!("cat_{}={}\n", i, CategoryRules::join_list(&c.keywords)));
        }
        if let Err(e) = std::fs::write(Self::path(dir), text) {
            eprintln!("settings 写入失败：{e}");
        }
    }
}

/// 行内编辑会话（EDIT 覆盖层）
pub struct EditSession {
    pub hwnd_edit: HWND,
    pub old_proc: isize,
    pub row: usize,
    pub scheme: usize,
    pub entry: usize,
    pub col: usize,
}

pub struct App {
    pub doc: Doc,
    pub path: PathBuf,
    pub dirty: bool,
    /// 右表行号 → (方案下标, 条目下标)，全局平铺（应用过滤后仅含匹配行）
    pub row_map: Vec<(usize, usize)>,
    pub selected_cells: HashSet<(usize, usize)>,
    pub hwnd_main: HWND,
    /// 工具栏自绘按钮（hwnd, id)
    pub hwnd_tbtns: Vec<(HWND, i32)>,
    /// 状态筛选 chips（hwnd, id)
    pub hwnd_chips: Vec<(HWND, i32)>,
    pub hwnd_sort: HWND,
    pub hwnd_status: HWND,
    pub hwnd_records: HWND,
    pub hwnd_header: HWND,
    pub header_old_proc: isize,
    pub hwnd_search: HWND,
    /// 底部状态条文本（上次保存等）
    pub status_msg: String,
    pub last_save: Option<String>,
    pub edit: Option<EditSession>,
    /// 刷新/弹层过程中的通知防重入
    pub busy: bool,
    /// 冲突缓存与 (方案, 条目) → 角色 映射
    pub conflicts: Vec<ConflictGroup>,
    pub roles: HashMap<(usize, usize), ConflictRole>,
    /// 搜索过滤词（小写）
    pub filter: String,
    pub filter_state: FilterState,
    pub category: CatFilter,
    /// 各分类记录数，长度 = 分类数 + 1（末位 = 兜底「自定义」）
    pub category_counts: Vec<usize>,
    pub sort_mode: SortMode,
    pub count_enabled: usize,
    pub count_disabled: usize,
    pub count_conflict: usize,
    /// 外部修改签名（mtime 秒, 长度）
    pub ext_sig: Option<(i64, u64)>,
    pub settings: Settings,
    /// 数据目录（便携模式 = exe 旁 HostsMate.data，否则 %APPDATA%\HostsMate）
    pub data_dir: PathBuf,
    pub backup_dir: PathBuf,
    pub is_admin: bool,
    pub font: HFONT,
    pub header_font: HFONT,
    pub sub_font: HFONT,
    pub btn_dcs: Vec<(windows::Win32::Graphics::Gdi::HDC, windows::Win32::Graphics::Gdi::HBITMAP)>,
    pub op_dcs: Vec<(windows::Win32::Graphics::Gdi::HDC, windows::Win32::Graphics::Gdi::HBITMAP)>,
    pub cat_dcs: Vec<(windows::Win32::Graphics::Gdi::HDC, windows::Win32::Graphics::Gdi::HBITMAP)>,
}

pub fn data_dir() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.to_path_buf()));
    if let Some(d) = &exe_dir {
        let portable = d.join("HostsMate.data");
        if portable.is_dir() {
            return portable;
        }
    }
    std::env::var_os("APPDATA")
        .map(|a| PathBuf::from(a).join("HostsMate"))
        .or_else(|| exe_dir.map(|d| d.join("HostsMate.data")))
        .unwrap_or_else(|| PathBuf::from("."))
}

impl App {
    pub fn new(path: PathBuf) -> Self {
        let dir = data_dir();
        let backup_dir = dir.join("backups");
        let settings = Settings::load(&dir);
        Self {
            doc: Doc {
                prelude: Vec::new(),
                schemes: Vec::new(),
            },
            path,
            dirty: false,
            row_map: Vec::new(),
            selected_cells: HashSet::new(),
            hwnd_main: HWND::default(),
            hwnd_tbtns: Vec::new(),
            hwnd_chips: Vec::new(),
            hwnd_sort: HWND::default(),
            hwnd_status: HWND::default(),
            hwnd_records: HWND::default(),
            hwnd_header: HWND::default(),
            header_old_proc: 0,
            hwnd_search: HWND::default(),
            status_msg: String::new(),
            last_save: None,
            edit: None,
            busy: false,
            conflicts: Vec::new(),
            roles: HashMap::new(),
            filter: String::new(),
            filter_state: FilterState::All,
            category: CatFilter::All,
            category_counts: vec![0; 5],
            sort_mode: SortMode::FileOrder,
            count_enabled: 0,
            count_disabled: 0,
            count_conflict: 0,
            ext_sig: None,
            settings,
            data_dir: dir,
            backup_dir,
            is_admin: false,
            font: HFONT::default(),
            header_font: HFONT::default(),
            sub_font: HFONT::default(),
            btn_dcs: Vec::new(),
            op_dcs: Vec::new(),
            cat_dcs: Vec::new(),
        }
    }

    /// 从磁盘读取（解析不了的行由引擎原样保留）
    pub fn load_from_disk(&mut self) -> Result<(), String> {
        let bytes = std::fs::read(&self.path).map_err(|e| format!("读取失败：{e}"))?;
        self.doc = engine::parse_bytes(&bytes);
        self.dirty = false;
        self.edit = None;
        self.ext_sig = self.ext_sig_of(&self.path);
        self.last_save = file_time_label(&self.path);
        Ok(())
    }

    /// 文件签名（mtime 秒, 长度）
    pub fn ext_sig_of(&self, path: &Path) -> Option<(i64, u64)> {
        let md = std::fs::metadata(path).ok()?;
        let secs = md
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs() as i64;
        Some((secs, md.len()))
    }

    /// 冲突缓存重算（规格 §2.3：仅有效启用记录参与，文件顺序首条生效）
    pub fn refresh_conflicts(&mut self) {
        self.conflicts = engine::detect_conflicts(&self.doc);
        self.roles.clear();
        for g in &self.conflicts {
            for (i, m) in g.members.iter().enumerate() {
                let role = match g.kind {
                    ConflictKind::Real => {
                        if m.effective {
                            ConflictRole::RealEffective
                        } else {
                            ConflictRole::RealOverridden
                        }
                    }
                    ConflictKind::Redundant => {
                        if i == 0 {
                            ConflictRole::RedundantKept
                        } else {
                            ConflictRole::RedundantExtra
                        }
                    }
                };
                self.roles.insert((m.scheme, m.entry), role);
            }
        }
        self.count_conflict = self.roles.len();
        let mut en = 0usize;
        let mut dis = 0usize;
        for scheme in &self.doc.schemes {
            for e in &scheme.entries {
                if let Entry::Record(r) = e {
                    if r.enabled {
                        en += 1;
                    } else {
                        dis += 1;
                    }
                }
            }
        }
        self.count_enabled = en;
        self.count_disabled = dis;
        self.category_counts = vec![0; self.settings.rules.cats.len() + 1];
        for scheme in &self.doc.schemes {
            for entry in &scheme.entries {
                if let Entry::Record(r) = entry {
                    let i = self.settings.rules.classify(r);
                    self.category_counts[i] += 1;
                }
            }
        }
    }

    /// 保存前备份当前文件（单文件模型下承担数据库级重要性）
    pub fn backup_now(&self) -> Result<PathBuf, String> {
        if !self.path.exists() {
            return Ok(PathBuf::new());
        }
        engine::snapshot_backup(&self.path, &self.backup_dir, self.settings.backup_keep as usize)
            .map_err(|e| format!("备份失败：{e}"))
    }

    /// 直接原子写盘（管理员或非系统文件时使用）
    pub fn write_direct(&self) -> Result<(), String> {
        engine::save_doc(&self.path, &self.doc).map_err(|e| format!("保存失败：{e}"))
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn record(&self, scheme: usize, entry: usize) -> Option<&Record> {
        match self.doc.schemes.get(scheme)?.entries.get(entry)? {
            Entry::Record(r) => Some(r),
            Entry::Verbatim(_) => None,
        }
    }

    pub fn record_mut(&mut self, scheme: usize, entry: usize) -> Option<&mut Record> {
        match self.doc.schemes.get_mut(scheme)?.entries.get_mut(entry)? {
            Entry::Record(r) => Some(r),
            Entry::Verbatim(_) => None,
        }
    }

    pub fn record_by_row(&self, row: usize) -> Option<&Record> {
        let (s, e) = *self.row_map.get(row)?;
        self.record(s, e)
    }

    /// 在文件末尾的方案块追加记录；无任何块时建隐式「默认」块（不写组头）
    pub fn add_record(&mut self) -> (usize, usize) {
        if self.doc.schemes.is_empty() {
            self.doc.schemes.push(Scheme {
                name: DEFAULT_SCHEME.to_string(),
                explicit: false,
                entries: Vec::new(),
            });
        }
        let idx = self.doc.schemes.len() - 1;
        self.doc.schemes[idx]
            .entries
            .push(Entry::Record(Record {
                ip: "127.0.0.1".parse().unwrap(),
                domain: "new.local".to_string(),
                comment: None,
                enabled: true,
            }));
        self.mark_dirty();
        (idx, self.doc.schemes[idx].entries.len() - 1)
    }

    /// 删除全局平铺的记录（跨方案；按 (方案, 条目) 降序安全移除）
    pub fn delete_rows(&mut self, mut cells: Vec<(usize, usize)>) {
        self.selected_cells.clear();
        cells.sort_unstable_by(|a, b| b.cmp(a));
        for (s, e) in cells {
            if let Some(scheme) = self.doc.schemes.get_mut(s) {
                if e < scheme.entries.len() {
                    scheme.entries.remove(e);
                }
            }
        }
        self.mark_dirty();
    }

    /// 删除单条记录（操作列垃圾桶，无确认）
    pub fn delete_one(&mut self, cell: (usize, usize)) {
        if let Some(scheme) = self.doc.schemes.get_mut(cell.0) {
            if cell.1 < scheme.entries.len() {
                scheme.entries.remove(cell.1);
            }
        }
        self.mark_dirty();
    }

    /// 置顶生效：把 (scheme, entry) 的真冲突记录移到其域名冲突组最前（成为生效行）
    pub fn pin_to_top(&mut self, scheme: usize, entry: usize) -> bool {
        let Some(group) = self.conflicts.iter().find(|g| {
            g.kind == ConflictKind::Real
                && g.members.iter().any(|m| m.scheme == scheme && m.entry == entry)
        }) else {
            return false;
        };
        let first = (group.members[0].scheme, group.members[0].entry);
        if first == (scheme, entry) {
            return false; // 已是生效行
        }
        let moved = self.doc.schemes[scheme].entries.remove(entry);
        self.doc.schemes[first.0].entries.insert(first.1, moved);
        self.mark_dirty();
        true
    }

    /// 一键清理冗余：保留组内第一条，删除其余同域名同 IP 行，返回删除数
    pub fn cleanup_redundant(&mut self, group_idx: usize) -> usize {
        let Some(g) = self.conflicts.get(group_idx) else {
            return 0;
        };
        if g.kind != ConflictKind::Redundant {
            return 0;
        }
        let mut victims: Vec<(usize, usize)> = g
            .members
            .iter()
            .skip(1)
            .map(|m| (m.scheme, m.entry))
            .collect();
        // (方案, 条目) 降序删除，避免位移
        victims.sort_unstable_by(|a, b| b.cmp(a));
        let n = victims.len();
        for (s, e) in victims {
            if let Some(scheme) = self.doc.schemes.get_mut(s) {
                if e < scheme.entries.len() {
                    scheme.entries.remove(e);
                }
            }
        }
        self.mark_dirty();
        n
    }

    /// 记录是否匹配搜索词（域名/IP/备注子串，大小写不敏感）
    pub fn text_matches(&self, r: &Record, filter: &str) -> bool {
        if filter.is_empty() {
            return true;
        }
        r.domain.to_lowercase().contains(filter)
            || r.ip.to_string().contains(filter)
            || r.comment
                .as_deref()
                .map_or(false, |c| c.to_lowercase().contains(filter))
    }

    /// 记录是否匹配当前筛选（搜索词 + 状态 chip）
    pub fn record_matches(&self, r: &Record, cell: (usize, usize), filter: &str) -> bool {
        if !self.text_matches(r, filter) {
            return false;
        }
        if let CatFilter::Cat(i) = self.category {
            if self.settings.rules.classify(r) != i {
                return false;
            }
        }
        match self.filter_state {
            FilterState::All => true,
            FilterState::Enabled => r.enabled,
            FilterState::Disabled => !r.enabled,
            FilterState::Conflicted => self.roles.contains_key(&cell),
        }
    }
}

pub fn file_time_label(path: &Path) -> Option<String> {
    let ticks = std::fs::metadata(path).ok()?.last_write_time();
    let file = FILETIME { dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32 };
    let mut local = FILETIME::default();
    let mut time = SYSTEMTIME::default();
    unsafe {
        FileTimeToLocalFileTime(&file, &mut local).ok()?;
        FileTimeToSystemTime(&local, &mut time).ok()?;
    }
    Some(format!("{:04}/{:02}/{:02} {:02}:{:02}", time.wYear,
        time.wMonth, time.wDay, time.wHour, time.wMinute))
}

#[cfg(test)]
mod rules_tests {
    use super::*;

    fn rec(domain: &str, note: Option<&str>) -> Record {
        Record {
            ip: "0.0.0.0".parse().unwrap(),
            domain: domain.to_string(),
            comment: note.map(|s| s.to_string()),
            enabled: true,
        }
    }

    /// 默认规则必须复现旧的硬编码分类行为（下标：CDN=0 分析=1 跟踪=2 广告=3 兜底=4）
    #[test]
    fn default_rules_match_legacy_behavior() {
        let d = CategoryRules::default();
        // CDN 优先级最高：域名含 cdn、备注含 静态资源，均压过广告备注/xunlei 域名
        assert_eq!(d.classify(&rec("88cdn.com", Some("广告网络"))), 0);
        assert_eq!(d.classify(&rec("admin.static.xl9.xunlei.com", Some("迅雷-静态资源"))), 0);
        // 分析：备注或域名关键词
        assert_eq!(d.classify(&rec("advstat.xunlei.com", Some("业务分析"))), 1);
        assert_eq!(d.classify(&rec("analysis-acc-ssl.xunlei.com", Some("数据分析"))), 1);
        assert_eq!(d.classify(&rec("biz5.sandai.net", Some("业务分析"))), 1);
        // 跟踪
        assert_eq!(d.classify(&rec("tracker.net", None)), 2);
        assert_eq!(d.classify(&rec("eye.example.com", Some("用户埋点"))), 2);
        // 广告：备注关键词 + 域名前缀/包含
        assert_eq!(d.classify(&rec("ad.example.com", None)), 3);
        assert_eq!(d.classify(&rec("adsp.example.com", None)), 3);
        assert_eq!(d.classify(&rec("act.niu.xunlei.com", Some("迅雷-活动"))), 3);
        assert_eq!(d.classify(&rec("advertpay.example.com", None)), 3);
        // 前缀语义精确：ad.* 不误伤 download.com，也不匹配 ads. 开头
        assert_eq!(d.classify(&rec("download.com", None)), 4);
        assert_eq!(d.classify(&rec("ads.example.com", None)), 4);
        // 兜底
        assert_eq!(d.classify(&rec("new.local", None)), 4);
    }

    /// 关键词解析：中英文逗号/顿号/分号分隔，去空项与首尾空白
    #[test]
    fn parse_list_handles_cjk_separators() {
        assert_eq!(
            CategoryRules::parse_list("广告， ad.* 、track；xunlei ,,"),
            vec!["广告".to_string(), "ad.*".to_string(), "track".to_string(), "xunlei".to_string()]
        );
        assert!(CategoryRules::parse_list(" , ，、；").is_empty());
    }

    /// 自定义分类生效；清空某类关键词 = 该类不再命中；删分类后规则可整列表替换
    #[test]
    fn custom_and_cleared_rules() {
        let mut r = CategoryRules::default();
        r.cats[3].keywords = vec!["业务".into()];
        assert_eq!(r.classify(&rec("shop.example.com", Some("业务专用"))), 3);
        r.cats[3].keywords = Vec::new();
        assert_eq!(r.classify(&rec("ads.example.com", Some("广告"))), 4);
        // 用户删到只剩一个分类：兜底 = 1
        r.cats.truncate(1);
        assert_eq!(r.classify(&rec("ad.example.com", None)), 1);
    }

    /// settings.ini 往返：缺省键=默认规则；新增/改名分类与关键词读回一致；空值=清空
    #[test]
    fn settings_roundtrip() {
        let dir = std::env::temp_dir().join(format!("hostsmate_test_rules_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        // 无文件 → 默认
        let fresh = Settings::load(&dir);
        assert_eq!(fresh.rules, CategoryRules::default());
        // 修改 + 新增分类 → 读回一致
        let mut s = Settings::default();
        s.rules.cats[0].keywords = vec!["镜像".to_string()];
        s.rules.cats[3].keywords = Vec::new();
        s.rules.cats.push(UserCategory {
            name: "游戏".to_string(),
            keywords: vec!["game".to_string(), "play.*".to_string()],
        });
        s.store(&dir);
        let loaded = Settings::load(&dir);
        assert_eq!(loaded.rules.cats.len(), 5);
        assert_eq!(loaded.rules.cats[0].keywords, vec!["镜像".to_string()]);
        assert!(loaded.rules.cats[3].keywords.is_empty());
        assert_eq!(loaded.rules.cats[1].keywords, CategoryRules::default().cats[1].keywords);
        assert_eq!(loaded.rules.cats[4].name, "游戏");
        assert_eq!(loaded.rules.cats[4].keywords, vec!["game".to_string(), "play.*".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 旧版 settings.ini（cat_cdn 等四键）无损迁移为默认四类
    #[test]
    fn legacy_ini_migration() {
        let dir = std::env::temp_dir().join(format!("hostsmate_test_legacy_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(Settings::path(&dir),
            "flushdns=0\nbackup_keep=7\ncat_cdn=镜像\ncat_tracking=\n").unwrap();
        let s = Settings::load(&dir);
        assert!(!s.flushdns);
        assert_eq!(s.backup_keep, 7);
        assert_eq!(s.rules.cats.len(), 4);
        assert_eq!(s.rules.cats.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            vec!["CDN", "分析", "跟踪", "广告"]);
        assert_eq!(s.rules.cats[0].keywords, vec!["镜像".to_string()]);
        assert!(s.rules.cats[2].keywords.is_empty());
        // 未写的键沿用默认关键词
        assert_eq!(s.rules.cats[3].keywords, CategoryRules::default().cats[3].keywords);
        // 迁移后再存读，走新格式且内容不丢
        s.store(&dir);
        let again = Settings::load(&dir);
        assert_eq!(again.rules, s.rules);
        assert_eq!(again.backup_keep, 7);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
