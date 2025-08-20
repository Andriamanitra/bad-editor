use syntect::dumps::dump_to_uncompressed_file;
use syntect::parsing::SyntaxSetBuilder;

fn main() {
    println!("cargo:rerun-if-changed=syntaxes");

    let out_dir = std::env::var("OUT_DIR").unwrap();

    let mut syntax_set_builder = SyntaxSetBuilder::new();
    syntax_set_builder.add_from_folder("syntaxes", true).unwrap();
    syntax_set_builder.add_plain_text_syntax();
    let syntax_set = syntax_set_builder.build();
    dump_to_uncompressed_file(&syntax_set, format!("{out_dir}/syntaxes.packdump"))
        .expect("dumping syntaxes should work");
}
