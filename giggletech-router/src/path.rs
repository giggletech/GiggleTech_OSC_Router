use std::env;
use std::path::{PathBuf, Path};
use std::error::Error;

/// Gets the directory of the current executable.
///
/// # Returns
/// - `Ok(PathBuf)` containing the directory path of the current executable.
/// - `Err(Box<dyn Error>)` if the directory cannot be determined or another error occurs.
pub fn current_exe_dir() -> Result<PathBuf, Box<dyn Error>> {
    let exe_path = env::current_exe()?;
    let dir = exe_path.parent().ok_or("Could not determine the executable's directory")?;
    Ok(dir.to_path_buf())
}

/// Converts a `Path` to a `&str`, checking for valid Unicode.
///
/// # Parameters
/// - `path`: The `Path` to convert.
///
/// # Returns
/// - `Ok(&str)` containing the path as a string.
/// - `Err(Box<dyn Error>)` if the path contains invalid Unicode characters.
pub fn path_to_str(path: &Path) -> Result<&str, Box<dyn Error>> {
    path.to_str().ok_or_else(|| "Path contains invalid Unicode characters".into())
}

/// Joins the current executable directory with a specified filename, returning the full path.
///
/// # Parameters
/// - `filename`: The name of the file to join with the executable directory.
///
/// # Returns
/// - `Ok(PathBuf)` containing the full path to the file.
/// - `Err(Box<dyn Error>)` if an error occurs during the operation.
pub fn join_exe_dir_with_file(filename: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = current_exe_dir()?;
    Ok(dir.join(filename))
}