use std::{fs::{read_dir, read_to_string, write}, path::Path};

use crate::{parser::qml::emitter::{emit, flatten_lines, Line}, util::common_util::parse_qml};

fn destroy_indents(lines: &mut Vec<Line>) {
    lines.iter_mut().for_each(|e| e.indent = 0);
}

// Parse the file first, then emit it.
// After that, take the emitted file and parse it again and emit once more.
// If the parser and emitter work, the last-emitted file and the one emitted
// before it should match perfectly.
// String (with pretty formatting) -> AST -> String (with no formatting) -> AST -> String (with no formatting)
fn test_qml_parser_on_file(file: &Path) {
    let contents = read_to_string(file).unwrap();
    print!("Testing the qml parser on file: {}... ", file.display());
    let ast_first_pass = parse_qml(contents, file.to_str().unwrap(), None, None).unwrap();
    let mut lines_first_emit = emit(&ast_first_pass);
    destroy_indents(&mut lines_first_emit);
    let emit_first_pass = flatten_lines(&lines_first_emit).replace(" instanceof ", "instanceof").replace(" new ", "new");
    let ast_second_pass = parse_qml(emit_first_pass.clone(), file.to_str().unwrap(), None, None).unwrap();
    let mut lines_second_emit = emit(&ast_second_pass);
    destroy_indents(&mut lines_second_emit);
    let emit_second_pass = flatten_lines(&lines_second_emit).replace(" instanceof ", "instanceof").replace(" new ", "new");
    if emit_first_pass != emit_second_pass {
        println!("ERROR!");
        println!("First pass:\n{}", emit_first_pass);
        println!("------------\nSecond pass:\n{}", emit_second_pass);
        let root = Path::new(OUTPUT_DIR);
        write(root.join("E1"), emit_first_pass).unwrap();
        write(root.join("E2"), emit_second_pass).unwrap();
        panic!();
    }
    println!("OK!");
}

const TEST_DIR: &'static str = "/ram/test_qml_root";
const OUTPUT_DIR: &'static str = "/ram/";

fn test_recursively(dir: &Path) {
    println!("Recursing into {}...", dir.display());
    for entry in read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            test_recursively(entry.path().as_path());
        } else if entry.file_name().to_str().unwrap().to_lowercase().ends_with(".qml") {
            test_qml_parser_on_file(entry.path().as_path());
        }
    }
}

#[test]
fn test_qml_parser_recursively() {
    test_recursively(Path::new(TEST_DIR));
}
