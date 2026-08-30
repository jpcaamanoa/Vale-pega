use serde::Serialize;

#[derive(Serialize)]
pub struct AppInfo {
    name: String,
    version: String,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: env!("CARGO_PKG_NAME").to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_actual_package_name_and_version() {
        let info = app_info();
        assert_eq!(info.name, "cuaderno-clinico");
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    }
}
