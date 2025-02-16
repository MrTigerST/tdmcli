use std::env;
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use rayon::prelude::*;
use indicatif::{ProgressBar, ProgressStyle};

use base64::{engine::general_purpose, Engine as _};

const VERSION: &str = "1.0";
const KEY: &str = "tdmcliKeyy";

fn get_config_file_path() -> PathBuf {
    let mut config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    config_dir.push("tdmcli");
    fs::create_dir_all(&config_dir).unwrap_or_default();
    config_dir.push("config.toml");
    config_dir
}


fn read_config_template_dir() -> Option<PathBuf> {
    let config_file = get_config_file_path();
    if config_file.exists() {
        let contents = fs::read_to_string(config_file).ok()?;
        for line in contents.lines() {
            let line = line.trim();
            if line.starts_with("template_dir") {
                if let Some(pos) = line.find('=') {
                    let value = line[pos+1..].trim().trim_matches('"');
                    if !value.is_empty() {
                        return Some(PathBuf::from(value));
                    }
                }
            }
        }
    }
    None
}

fn change_template_dir(new_dir: &Path) {
    if !new_dir.exists() {
        fs::create_dir_all(new_dir).unwrap_or_else(|_| {
            println!("Failed to create directory {:?}", new_dir);
            std::process::exit(1);
        });
    }
    let config_file = get_config_file_path();
    let config_contents = format!("template_dir = \"{}\"\n", new_dir.to_string_lossy());
    fs::write(&config_file, config_contents).unwrap();
    println!("Template directory changed to {:?}", new_dir);
}

fn load_ignore_patterns(root_dir: &Path) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    let ignore_file = root_dir.join(".tdmignore");

    if let Ok(contents) = fs::read_to_string(&ignore_file) {
        for line in contents.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let mut pattern = trimmed.to_string();

            if pattern.starts_with('/') {
                pattern.remove(0);
            }

            if pattern.ends_with('/') {
                let dir_pattern = pattern.trim_end_matches('/').to_string();
                builder.add(Glob::new(&dir_pattern).unwrap());
                let wildcard_pattern = format!("{}**", pattern);
                builder.add(Glob::new(&wildcard_pattern).unwrap());
            } else if pattern.contains('/') {

                builder.add(Glob::new(&pattern).unwrap());
                let wildcard_pattern = format!("{}/**", pattern);
                builder.add(Glob::new(&wildcard_pattern).unwrap());
            } else {
                builder.add(Glob::new(&pattern).unwrap());
            }
        }
    }
    builder.build().unwrap()
}

fn should_ignore(path: &Path, root_dir: &Path, patterns: &GlobSet, exclude_tdmignore: bool) -> bool {
    let relative_path = path.strip_prefix(root_dir).unwrap_or(path);

    if relative_path == Path::new(".tdmignore") {
        return exclude_tdmignore;
    }

    let relative_str = relative_path.to_str().unwrap_or("");
    if patterns.is_match(relative_str) {
        return true;
    }

    for component in relative_path.components() {
        if let Some(comp_str) = component.as_os_str().to_str() {
            if patterns.is_match(comp_str) {
                return true;
            }
        }
    }
    false
}


fn get_templates_dir() -> PathBuf {
    if let Ok(dir) = env::var("TDMCLI_TEMPLATE_DIR") {
        let path = PathBuf::from(dir);
        fs::create_dir_all(&path).unwrap_or_default();
        return path;
    }
    if let Some(dir) = read_config_template_dir() {
        fs::create_dir_all(&dir).unwrap_or_default();
        return dir;
    }
    let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("tdmcli/templates");
    fs::create_dir_all(&path).unwrap_or_default();
    path
}

fn xor_crypt(data: &[u8], key: &str) -> Vec<u8> {
    let key_bytes = key.as_bytes();
    data.iter()
        .enumerate()
        .map(|(i, &b)| b ^ key_bytes[i % key_bytes.len()])
        .collect()
}

fn process_file(file_path: &Path, root_dir: &Path) -> (String, Vec<u8>) {
    let relative_path = file_path.strip_prefix(root_dir)
        .unwrap()
        .to_string_lossy()
        .to_string();
    let mut content = Vec::new();
    File::open(file_path).unwrap().read_to_end(&mut content).unwrap();
    let encrypted_content = xor_crypt(&content, KEY);
    (relative_path, encrypted_content)
}

fn is_in_hidden_directory(path: &Path, root_dir: &Path) -> bool {
    if let Ok(relative) = path.strip_prefix(root_dir) {
        if let Some(parent) = relative.parent() {
            return parent.components().any(|comp| {
                comp.as_os_str()
                    .to_str()
                    .map(|s| s.starts_with('.'))
                    .unwrap_or(false)
            });
        }
    }
    false
}

fn create_template(template_name: &str, root_dir: &Path, include_hidden: bool, exclude_ignore: bool) {
    println!("Loading... Creating template '{}'.", template_name);
    let template_path = get_templates_dir().join(format!("{}.tdmcli", template_name));
    let ignore_patterns = load_ignore_patterns(root_dir);

    let file_entries: Vec<PathBuf> = walkdir::WalkDir::new(root_dir)
    .into_iter()
    .filter_map(|entry| entry.ok())
    .filter(|entry| {
        let path = entry.path();

        if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
            if file_name == ".tdmignore" {
                return !exclude_ignore;
            }
        }

        if !include_hidden && is_in_hidden_directory(path, root_dir) {
            return false;
        }

        if should_ignore(path, root_dir, &ignore_patterns, exclude_ignore) {
            return false;
        }

        entry.file_type().is_file()
    })
    .map(|entry| entry.path().to_path_buf())
    .collect();

    let pb_files = ProgressBar::new(file_entries.len() as u64);
    pb_files.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} files")
        .unwrap());

    let file_results: Vec<(String, Vec<u8>)> = file_entries.par_iter()
        .map(|file| {
            let res = process_file(file, root_dir);
            pb_files.inc(1);
            res
        })
        .collect();
    pb_files.finish_with_message("File processing complete");

    let empty_dirs: Vec<PathBuf> = walkdir::WalkDir::new(root_dir)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return false;
            }

            if !include_hidden {
                if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                    if name.starts_with('.') {
                        return false;
                    }
                }
            }

            if should_ignore(path, root_dir, &ignore_patterns, exclude_ignore) {
                return false;
            }

            fs::read_dir(path)
                .map(|iter| {
                    iter.filter(|entry| {
                        if let Ok(entry) = entry {
                            if !include_hidden {
                                if let Some(name) = entry.file_name().to_str() {
                                    return !name.starts_with('.');
                                }
                            }
                            true
                        } else {
                            false
                        }
                    }).next().is_none()
                })
                .unwrap_or(false)
        })
        .map(|entry| entry.path().to_path_buf())
        .collect();

    let mut template_file = File::create(&template_path).unwrap();

    for dir in empty_dirs {
        let relative_path = dir.strip_prefix(root_dir)
            .unwrap()
            .to_string_lossy()
            .to_string();
        if relative_path.is_empty() {
            continue;
        }
        writeln!(template_file, "DIR: {}", relative_path).unwrap();
        writeln!(template_file, "END_OF_DIR").unwrap();
    }

    for (relative_path, encrypted_content) in file_results {
        let encoded = general_purpose::STANDARD.encode(&encrypted_content);
        writeln!(template_file, "FILE: {}", relative_path).unwrap();
        writeln!(template_file, "SIZE: {}", encoded.len()).unwrap();
        writeln!(template_file, "{}", encoded).unwrap();
        writeln!(template_file, "END_OF_FILE").unwrap();
    }

    println!("Template '{}' created successfully.", template_name);
}

fn apply_template(template_name: &str) {
    println!("Loading... Applying template '{}'.", template_name);
    let template_path = get_templates_dir().join(format!("{}.tdmcli", template_name));
    if !template_path.exists() {
        println!("Template '{}' not found.", template_name);
        return;
    }

    let content = fs::read_to_string(&template_path).unwrap();
    let mut file_entries = Vec::new();
    let mut dir_entries = Vec::new();
    let mut lines = content.lines();

    while let Some(line) = lines.next() {
        if line.starts_with("FILE: ") {
            let file_name = line[6..].to_string();
            let size_line = lines.next().unwrap();
            let size: usize = size_line[6..].parse().unwrap();
            let encoded = lines.next().unwrap().to_string();
            let _ = lines.next().unwrap();
            file_entries.push((file_name, size, encoded));
        } else if line.starts_with("DIR: ") {
            let dir_name = line[5..].to_string();
            let end_line = lines.next().unwrap();
            if end_line != "END_OF_DIR" {
                eprintln!("Template format error: expected END_OF_DIR after directory entry.");
            }
            dir_entries.push(dir_name);
        }
    }

    for dir_name in dir_entries {
        let path = Path::new(&dir_name);
        fs::create_dir_all(path).unwrap();
    }

    let pb = ProgressBar::new(file_entries.len() as u64);
    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} files")
        .unwrap());

    file_entries.par_iter().for_each(|(file_name, size, encoded)| {
        let encrypted_content = general_purpose::STANDARD.decode(encoded).unwrap();
        if encoded.len() != *size {
            eprintln!("Warning: the declared size does not match the encoded content for file {}", file_name);
        }
        let decrypted_content = xor_crypt(&encrypted_content, KEY);
        let path = Path::new(file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        File::create(path).unwrap().write_all(&decrypted_content).unwrap();
        pb.inc(1);
    });
    pb.finish_with_message("Template applied successfully");
}

fn delete_template(template_name: &str) {
    let template_path = get_templates_dir().join(format!("{}.tdmcli", template_name));
    if fs::remove_file(&template_path).is_ok() {
        println!("Template '{}' deleted successfully.", template_name);
    } else {
        println!("Template '{}' not found.", template_name);
    }
}

fn show_template_directory() {
    let dir = get_templates_dir();
    println!("Templates directory: {}", dir.display());
}

fn list_templates() {
    let templates_dir = get_templates_dir();
    let templates: Vec<_> = fs::read_dir(&templates_dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension()
            .map(|ext| ext == "tdmcli")
            .unwrap_or(false))
        .map(|entry| entry.file_name().into_string().unwrap().replace(".tdmcli", ""))
        .collect();

    if templates.is_empty() {
        println!("No templates found.");
    } else {
        println!("Available templates:");
        for template in templates {
            println!("- {}", template);
        }
    }
}

fn show_version() {
    println!("tdmcli version: {}", VERSION);
}

fn export_template(template_name: &str, output_dir: &Path) {
    let template_path = get_templates_dir().join(format!("{}.tdmcli", template_name));
    if template_path.exists() {
        fs::create_dir_all(output_dir).unwrap();
        fs::copy(&template_path, output_dir.join(format!("{}.tdmcli", template_name))).unwrap();
        println!("Template '{}' exported to '{:?}'", template_name, output_dir);
    } else {
        println!("Template '{}' not found.", template_name);
    }
}

fn import_template(input_file: &Path, template_name: Option<&str>) {
    let template_name = template_name.unwrap_or_else(|| {
        input_file.file_stem().unwrap().to_str().unwrap()
    });
    let dest_path = get_templates_dir().join(format!("{}.tdmcli", template_name));
    fs::copy(input_file, &dest_path).unwrap();
    println!("Template imported from '{:?}' as '{}'", input_file, template_name);
}

fn get_latest_release_version() -> Option<String> {
    let url = "https://raw.githubusercontent.com/MrTigerST/tdmcli/main/version";
    match reqwest::blocking::get(url) {
        Ok(response) => match response.text() {
            Ok(text) => Some(text.trim().to_string()),
            Err(_) => None,
        },
        Err(_) => None,
    }
}

fn check_for_updates() {
    if let Some(latest_version) = get_latest_release_version() {
        println!("Latest version available: {}", latest_version);
        println!("Your current version: {}", VERSION);
        if latest_version != VERSION {
            println!("A new version is available! Download it from GitHub.");
        } else {
            println!("You are using the latest version.");
        }
    } else {
        println!("Failed to check for updates.");
    }
}

fn check_for_update_normalize() {
    if let Some(latest_version) = get_latest_release_version() {
        if latest_version != VERSION {
            println!("A new version is available! Download it from GitHub.");
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!(r#"Usage: tdmcli <command> [arguments]

Examples:
  tdmcli create <template_name> [--hiddenfolder] [--excludeignore]   Create a template (include hidden folders if flag provided, exclude .tdmignore if flag provided).
  tdmcli get <template_name>       Apply the template.
  tdmcli delete <template_name>    Delete a template.
  tdmcli list                      Show all templates.
  tdmcli import <input_file> [template_name]      Import an external template.
  tdmcli export <template_name> <output_dir>        Export template.
  tdmcli show-dir                  Show the directory where templates are stored.
  tdmcli change-dir <new_directory>   Change template directory.
  tdmcli -v                        Show the current version.
  tdmcli -u                        Check for updates.
  tdmcli help                      Show this help."#);
        return;
    }

    if args[1].ends_with(".tdmcli") {
        println!("Detected .tdmcli file, importing...");
        import_template(Path::new(&args[1]), None);
        return;
    }

    match args[1].as_str() {
        "create" if args.len() >= 3 => {
            let include_hidden = args.iter().any(|arg| arg == "--hiddenfolder");
            let exclude_ignore = args.iter().any(|arg| arg == "--excludeignore");
            check_for_update_normalize();
            create_template(&args[2], &env::current_dir().unwrap(), include_hidden, exclude_ignore)
        }
        "get" if args.len() == 3 => {
            check_for_update_normalize();
            apply_template(&args[2])
        }
        "delete" if args.len() == 3 => {
            check_for_update_normalize();
            delete_template(&args[2])
        }
        "list" => {
            list_templates()
        }
        "export" if args.len() == 4 => {
            check_for_update_normalize();
            export_template(&args[2], Path::new(&args[3]))
        }
        "import" if args.len() >= 3 => {
            check_for_update_normalize();
            import_template(Path::new(&args[2]), args.get(3).map(String::as_str))
        }
        "show-dir" => {
            show_template_directory();
        }
        "change-dir" if args.len() == 3 => {
            let new_dir = Path::new(&args[2]);
            change_template_dir(new_dir);
        }
        "version" | "-v" => {
            show_version()
        }
        "-u" => {
            check_for_updates()
        }
        _ => println!(r#"Usage: tdmcli <command> [arguments]

Examples:
  tdmcli create <template_name> [--hiddenfolder] [--excludeignore]   Create a template (include hidden folders if flag provided, exclude .tdmignore if flag provided).
  tdmcli get <template_name>       Apply the template.
  tdmcli delete <template_name>    Delete a template.
  tdmcli list                      Show all templates.
  tdmcli import <input_file> [template_name]      Import an external template.
  tdmcli export <template_name> <output_dir>        Export template.
  tdmcli show-dir                  Show the directory where templates are stored.
  tdmcli change-dir <new_directory>   Change template directory.
  tdmcli -v                        Show the current version.
  tdmcli -u                        Check for updates.
  tdmcli help                      Show this help."#),
    }
}