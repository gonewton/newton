use std::path::Path;

use utoipa::OpenApi;

fn main() -> anyhow::Result<()> {
    let doc = newton::api::openapi::ApiDoc::openapi();
    let yaml = serde_yaml::to_string(&doc)?;
    let path = Path::new("openapi/newton-backend-parity.yaml");
    std::fs::write(path, yaml)?;
    println!("wrote {}", path.display());
    Ok(())
}
