use ec4rs::PropertiesSource;
use syntect::dumps::dump_to_uncompressed_file;
use syntect::parsing::SyntaxSetBuilder;

fn main() {
    println!("cargo:rerun-if-changed=syntaxes");

    let out_dir = std::env::var("OUT_DIR").unwrap();

    ec4rs::ConfigFile::open("default_config/editorconfig")
        .expect("default_config/editorconfig should be valid editorconfig file")
        .apply_to(&mut ec4rs::Properties::default(), std::path::PathBuf::default())
        .expect("default_config/editorconfig should be valid editorconfig file");

    let mut syntax_set_builder = SyntaxSetBuilder::new();
    syntax_set_builder.add_from_folder("syntaxes", true).unwrap();
    syntax_set_builder.add_plain_text_syntax();
    let syntax_set = syntax_set_builder.build();
    dump_to_uncompressed_file(&syntax_set, format!("{out_dir}/syntaxes.packdump"))
        .expect("dumping syntaxes should work");
}
