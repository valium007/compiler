use anyhow::Result;
pub mod json;
pub use json::Program;

pub fn parse_json(json_content: &str) -> Result<Program> {
    let program: Program = serde_json::from_str(json_content)?;
    Ok(program)
}
