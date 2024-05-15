use cfg_if::cfg_if;
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const BIN_NAME: &str = env!("CARGO_PKG_NAME");
const APP_NAME: &str = "TetaNES";
const DIST_DIR: &str = "dist";
const TARGET_ARCH: &str = if cfg!(target_arch = "x86_64") {
    "x86_64"
} else if cfg!(target_arch = "aarch64") {
    "aarch64"
} else {
    panic!("unsupported target arch");
};

fn main() {
    let cargo_target_dir =
        PathBuf::from(env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string()));

    let dist_dir = PathBuf::from(DIST_DIR);
    let _ = fs::remove_dir_all(&dist_dir); // ignore if not found
    fs::create_dir_all(&dist_dir).expect("failed to create dist directory");

    cfg_if! {
        if #[cfg(target_os = "linux")] {
            make("build-all").expect("failed to build");
            create_linux_artifacts(&cargo_target_dir, &dist_dir).expect("failed to create linux artifacts");
            compress_web_artifacts(&dist_dir).expect("failed to compress web artifacts");
        } else if #[cfg(target_os = "macos")] {
            make("build").expect("failed to build");
            create_macos_app(&cargo_target_dir, &dist_dir).expect("failed to create macOS app");
        } else if #[cfg(target_os = "windows")] {
            create_windows_installer(&cargo_target_dir, &dist_dir).expect("failed to create windows installer");
        }
    }

    update_homebrew_formula(&cargo_target_dir).expect("failed to update homebrew formula");
}

/// Run `cargo make` to build binary.
///
/// Note: Wix on Windows bakes in the build step
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn make(cmd: &'static str) -> io::Result<()> {
    // TODO: disable lto and make pgo build
    Command::new("cargo").args(["make", cmd]).spawn()?.wait()?;

    Ok(())
}

/// Create a dist directory for artifacts.
fn create_build_dir(cargo_target_dir: &Path, subdir: impl AsRef<Path>) -> io::Result<PathBuf> {
    let subdir = subdir.as_ref();
    let build_dir = cargo_target_dir.join(subdir);

    println!("creating build directory: {build_dir:?}");

    let _ = fs::remove_dir_all(&build_dir); // ignore if not found
    fs::create_dir_all(&build_dir)?;

    Ok(build_dir)
}

/// Write out a SHA256 checksum for a file.
fn write_sha256(file: PathBuf, output: PathBuf) -> io::Result<()> {
    println!("writing sha256 for {file:?}");

    let shasum = {
        cfg_if! {
            if #[cfg(target_os = "windows")] {
                Command::new("powershell")
                    .arg("-Command")
                    .arg(format!("Get-FileHash -Algorithm SHA256 {} | select-object -ExpandProperty Hash", file.display()))
                    .output()?
            } else {
                Command::new("shasum")
                    .current_dir(file.parent().expect("parent directory"))
                    .args(["-a", "256"])
                    .arg(file.file_name().expect("filename"))
                    .output()?
            }
        }
    };
    let sha256 = std::str::from_utf8(&shasum.stdout)
        .expect("valid stdout")
        .trim()
        .to_owned();

    println!("sha256: {sha256}");

    fs::write(output, shasum.stdout)?;

    Ok(())
}

/// Update the homebrew formula.
fn update_homebrew_formula(_cargo_target_dir: &Path) -> io::Result<()> {
    println!("todo: update_homebrew_formula");

    Ok(())
}

/// Create linux artifacts.
#[cfg(target_os = "linux")]
fn create_linux_artifacts(cargo_target_dir: &Path, dist_dir: &Path) -> io::Result<()> {
    println!("creating linux artifacts...");

    let build_dir = create_build_dir(cargo_target_dir, "linux")?;

    println!("creating tarball...");

    fs::copy("README.md", build_dir.join("README.md"))?;
    fs::copy("LICENSE-MIT", build_dir.join("LICENSE-MIT"))?;
    fs::copy("LICENSE-APACHE", build_dir.join("LICENSE-APACHE"))?;
    fs::copy(
        cargo_target_dir.join("dist").join(BIN_NAME),
        build_dir.join(BIN_NAME),
    )?;

    let tgz_name = format!("{BIN_NAME}-{VERSION}-linux-{TARGET_ARCH}.tar.gz");
    Command::new("tar")
        .arg("-czvf")
        .arg(dist_dir.join(&tgz_name))
        .arg(format!("--directory={}", build_dir.display()))
        .arg(".")
        .spawn()?
        .wait()?;
    let tgz_sha_name = format!("{tgz_name}-sha256.txt");
    write_sha256(dist_dir.join(&tgz_name), dist_dir.join(&tgz_sha_name))?;

    println!("creating deb...");

    // NOTE: -1 is the deb revision number
    let deb_name = format!("{BIN_NAME}-{VERSION}-1-amd64.deb");
    Command::new("cargo")
        .args(["deb", "-p", "tetanes", "-o"])
        .arg(dist_dir.join(&deb_name))
        .spawn()?
        .wait()?;
    let deb_sha_name = format!("{deb_name}-sha256.txt");
    write_sha256(dist_dir.join(&deb_name), dist_dir.join(&deb_sha_name))?;

    println!("creating AppImage...");

    let app_dir = build_dir.join("AppDir");

    let app_image_name = format!("{APP_NAME}-{VERSION}-{TARGET_ARCH}.AppImage");
    Command::new(format!("vendored/linuxdeploy-{TARGET_ARCH}.AppImage"))
        .arg("-e")
        .arg(cargo_target_dir.join("dist").join(BIN_NAME))
        .arg("-i")
        .arg("assets/linux/icon.png")
        .arg("-d")
        .arg("assets/linux/TetaNES.desktop")
        .arg("--appdir")
        .arg(&app_dir)
        .arg("--output")
        .arg("appimage")
        .spawn()?
        .wait()?;

    fs::copy(
        format!("{APP_NAME}-{TARGET_ARCH}.AppImage"),
        dist_dir.join(&app_image_name),
    )?;
    let app_image_sha_name = format!("{app_image_name}-sha256.txt");
    write_sha256(
        dist_dir.join(&app_image_name),
        dist_dir.join(&app_image_sha_name),
    )?;

    println!("cleaning up...");

    let _ = fs::remove_file(format!("{APP_NAME}-{TARGET_ARCH}.AppImage"));

    Ok(())
}

/// Compress web artifacts.
fn compress_web_artifacts(dist_dir: &Path) -> io::Result<()> {
    println!("compressing web artifacts...");

    println!("creating tarball...");

    let tgz_name = format!("{BIN_NAME}-{VERSION}-web.tar.gz");
    Command::new("tar")
        .arg("-czvf")
        .arg(dist_dir.join(&tgz_name))
        .arg(format!("--directory={}", dist_dir.join("web").display()))
        .arg(".")
        .spawn()?
        .wait()?;
    let tgz_sha_name = format!("{tgz_name}-sha256.txt");
    write_sha256(dist_dir.join(&tgz_name), dist_dir.join(&tgz_sha_name))?;

    println!("cleaning up...");

    fs::remove_dir_all(dist_dir.join("web"))?;

    Ok(())
}

/// Create macOS app.
#[cfg(target_os = "macos")]
fn create_macos_app(cargo_target_dir: &Path, dist_dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::symlink;

    println!("creating macos app...");

    let build_dir = create_build_dir(cargo_target_dir, "macos")?;

    let artifact_name = format!("{APP_NAME}-{VERSION}-{TARGET_ARCH}");
    let volume = PathBuf::from("/Volumes").join(&artifact_name);
    let dmg_name = format!("{artifact_name}-Uncompressed.dmg");
    let dmg_name_compressed = format!("{artifact_name}.dmg");

    println!("creating dmg volume: {dmg_name}");

    let _ = Command::new("hdiutil").arg("detach").arg(&volume).status();
    Command::new("hdiutil")
        .args(["create", "-size", "50m", "-volname"])
        .arg(&artifact_name)
        .arg(build_dir.join(&dmg_name))
        .spawn()?
        .wait()?;
    Command::new("hdiutil")
        .arg("attach")
        .arg(build_dir.join(&dmg_name))
        .spawn()?
        .wait()?;

    println!("creating directories: {volume:?}");

    let app_dir = volume.join(format!("{APP_NAME}.app"));
    fs::create_dir_all(app_dir.join("Contents/MacOS"))?;
    fs::create_dir_all(app_dir.join("Contents/Resources"))?;
    fs::create_dir_all(volume.join(".Picture"))?;

    println!("updating Info.plist version: {VERSION:?}");

    let output = Command::new("sed")
        .arg("-e")
        .arg(format!("s/%VERSION%/{VERSION}/"))
        .arg("assets/macos/Info.plist")
        .output()?;
    fs::write(app_dir.join("Contents/Info.plist"), &output.stdout)?;

    println!("copying assets...");

    // TODO: maybe include readme/license?
    fs::copy(
        "assets/macos/Icon.icns",
        app_dir.join("Contents/Resources/Icon.icns"),
    )?;
    fs::copy(
        "assets/macos/background.png",
        volume.join(".Picture/background.png"),
    )?;
    fs::copy(
        cargo_target_dir.join("dist").join(BIN_NAME),
        app_dir.join("Contents/MacOS").join(BIN_NAME),
    )?;

    println!("creating /Applications symlink...");
    symlink("/Applications", volume.join("Applications"))?;

    println!("configuring app bundle window...");

    let configure_bundle_script = format!(
        r#"
        tell application "Finder"
            set f to POSIX file ("{volume}" as string) as alias
            tell folder f
                open
                    tell container window
                        set toolbar visible to false
                        set statusbar visible to false
                        set current view to icon view
                        delay 1 -- sync
                        set the bounds to {{0, 0, 720, 524}}
                    end tell
                    delay 1 -- sync
                    set icon size of the icon view options of container window to 120
                    set arrangement of the icon view options of container window to not arranged
                    set position of item ".Picture" to {{800, 320}}
                    set position of item ".fseventsd" to {{800, 320}}
                    set position of item "{APP_NAME}.app" to {{150, 300}}
                close
                set position of item "Applications" to {{425, 300}}
                open
                    set background picture of the icon view options of container window to file "background.png" of folder ".Picture"
                    set the bounds of the container window to {{0, 0, 600, 524}}
                    update without registering applications
                    delay 1 -- sync
                close
            end tell
            delay 1 -- sync
        end tell
    "#,
        volume = volume.display()
    );
    Command::new("osascript")
        .arg("-e")
        .arg(&configure_bundle_script)
        .spawn()?
        .wait()?;

    println!("signing code...");
    Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(app_dir.join("Contents/MacOS").join(BIN_NAME))
        .spawn()?
        .wait()?;
    // TODO: fix
    // ensure spctl --assess --type execute "${VOLUME}/${APP_NAME}.app"
    Command::new("codesign")
        .args(["--verify", "--strict", "--verbose=2"])
        .arg(app_dir.join("Contents/MacOS").join(BIN_NAME))
        .spawn()?
        .wait()?;

    println!("compressing dmg...");
    Command::new("hdiutil")
        .arg("detach")
        .arg(&volume)
        .spawn()?
        .wait()?;
    Command::new("hdiutil")
        .args(["convert", "-format", "UDBZ", "-o"])
        .arg(build_dir.join(&dmg_name_compressed))
        .arg(build_dir.join(&dmg_name))
        .spawn()?
        .wait()?;

    println!("writing artifacts...");

    fs::copy(
        build_dir.join(&dmg_name_compressed),
        dist_dir.join(&dmg_name_compressed),
    )?;
    let sha_name = format!("{artifact_name}-sha256.txt");
    write_sha256(dist_dir.join(&dmg_name_compressed), dist_dir.join(sha_name))?;

    println!("cleaning up...");

    fs::remove_file(build_dir.join(&dmg_name))?;

    Ok(())
}

/// Create Windows installer.
#[cfg(target_os = "windows")]
fn create_windows_installer(cargo_target_dir: &Path, dist_dir: &Path) -> io::Result<()> {
    println!("creating windows installer...");

    let build_dir = create_build_dir(cargo_target_dir, "wix")?;

    let artifact_name = format!("{BIN_NAME}-{VERSION}-{TARGET_ARCH}");
    let installer_name = format!("{artifact_name}.msi");

    println!("building installer...");

    Command::new("cargo")
        .args(["wix", "-p", "tetanes", "--nocapture"])
        .spawn()?
        .wait()?;

    println!("writing artifacts...");

    fs::copy(
        build_dir.join(&installer_name),
        dist_dir.join(&installer_name),
    )?;
    let sha_name = format!("{installer_name}-sha256.txt");
    write_sha256(dist_dir.join(&installer_name), dist_dir.join(&sha_name))?;

    Ok(())
}
