use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Runtime enum.
pub enum HostPlatform {
    /// Runtime enum variant.
    Posix,
    /// Runtime enum variant.
    Windows,
}

#[must_use]
/// Runtime function.
pub fn render_path_for_host(path: &Path, host: HostPlatform) -> String {
    let rendered = path.display().to_string();
    match host {
        HostPlatform::Posix => rendered.replace('\\', "/"),
        HostPlatform::Windows => rendered.replace('/', "\\"),
    }
}
