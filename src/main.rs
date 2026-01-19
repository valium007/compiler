pub mod ir;
pub mod parse;

fn main() {
    let _ = parse::parse_instructions();
}
