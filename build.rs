fn main() {
    // 内嵌图标 + comctl32 v6 / PerMonitorV2 manifest
    embed_resource::compile("app.rc", embed_resource::NONE);
}
