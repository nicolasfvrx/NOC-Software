use std::{env, fs, path::PathBuf, process::Command};

pub fn embed(product: &str) {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    assert_eq!(env::var("CARGO_CFG_TARGET_ENV").unwrap(), "msvc", "Windows metadata requires the MSVC Windows SDK");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let numeric = format!("{},{},{},0", env::var("CARGO_PKG_VERSION_MAJOR").unwrap(), env::var("CARGO_PKG_VERSION_MINOR").unwrap(), env::var("CARGO_PKG_VERSION_PATCH").unwrap());
    let binary = env::var("CARGO_PKG_NAME").unwrap();
    let description = env::var("CARGO_PKG_DESCRIPTION").unwrap();
    let resource = format!(r#"
1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0
FILEOS 0x40004L
FILETYPE 1
FILESUBTYPE 0
BEGIN
 BLOCK "StringFileInfo"
 BEGIN
  BLOCK "040904B0"
  BEGIN
   VALUE "CompanyName", "Norfair Operation Center\0"
   VALUE "FileDescription", "{product}\0"
   VALUE "FileVersion", "{version}\0"
   VALUE "InternalName", "{binary}\0"
   VALUE "OriginalFilename", "{binary}.exe\0"
   VALUE "ProductName", "{product}\0"
   VALUE "ProductVersion", "{version}\0"
   VALUE "Comments", "{description}\0"
  END
 END
 BLOCK "VarFileInfo"
 BEGIN
  VALUE "Translation", 0x0409, 1200
 END
END
"#);
    let source = out.join("version.rc");
    let compiled = out.join("version.res");
    fs::write(&source, resource).expect("Cannot write Windows version resource");
    let compiler = env::var_os("RC").map(PathBuf::from).unwrap_or_else(|| {
        let root = PathBuf::from(env::var_os("ProgramFiles(x86)").expect("Set RC to the Windows SDK rc.exe path"))
            .join("Windows Kits/10/bin");
        let mut versions: Vec<_> = fs::read_dir(root).expect("Windows SDK not installed; set RC")
            .filter_map(Result::ok).map(|entry| entry.path()).collect();
        versions.sort();
        versions.into_iter().rev().map(|path| path.join("x64/rc.exe"))
            .find(|path| path.is_file()).expect("Windows SDK rc.exe not found; set RC")
    });
    let status = Command::new(compiler).arg("/nologo").arg("/fo").arg(&compiled).arg(&source)
        .status().expect("Cannot run Windows resource compiler");
    assert!(status.success(), "Windows metadata compilation failed");
    println!("cargo:rustc-link-arg={}", compiled.display());
}
