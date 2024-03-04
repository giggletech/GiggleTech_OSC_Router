use std::env;
use std::path::{PathBuf, Path};
use std::error::Error;

// Gets the directory of the current executable.
pub fn current_exe_dir() -> Result<PathBuf, Box<dyn Error>> {
    let exe_path = env::current_exe()?;
    let dir = exe_path.parent().ok_or("Could not determine the executable's directory")?;
    Ok(dir.to_path_buf())
}

// Converts a `Path` to a `&str`, checking for valid Unicode.
pub fn path_to_str(path: &Path) -> Result<&str, Box<dyn Error>> {
    path.to_str().ok_or_else(|| "Path contains invalid Unicode characters".into())
}

// Joins the current executable directory with a specified filename, returning the full path.
pub fn join_exe_dir_with_file(filename: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = current_exe_dir()?;
    Ok(dir.join(filename))
}