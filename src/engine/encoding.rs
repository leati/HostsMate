//! 编码处理（规格 §2.1）：读 = BOM/UTF-8 检测，GBK 兜底；写 = 统一 UTF-8 无 BOM

/// 解码 hosts 文件字节：UTF-8 BOM → 去除；合法 UTF-8 → 直用；否则按 GBK 解码（非法字节替换）
pub fn decode_bytes(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => {
            let (cow, _enc, _had_errors) = encoding_rs::GBK.decode(bytes);
            cow.into_owned()
        }
    }
}

/// 输出统一 UTF-8 无 BOM
pub fn encode_utf8(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}
