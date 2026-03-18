use anyhow::Result;
pub mod json;
pub use json::Program;

pub fn parse_json() -> Result<Program> {
    let json = include_str!("../../out.json");
    let program: Program = serde_json::from_str(json)?;
    Ok(program)
}
