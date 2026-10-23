//! Gives `shot.exe` its icon: Explorer, shortcuts and the taskbar pin read it from the file.
//!
//! The icon goes into a compiled resource file (`.res`) written here, which the MSVC linker takes
//! as an input like an object file. No resource compiler is needed.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// The PNGs packed into the icon, in `assets/app/windows/`: flat up to 24 px, volumetric above.
const IMAGES: [&str; 8] = [
    "app-flat-16.png",
    "app-flat-20.png",
    "app-flat-24.png",
    "app-32.png",
    "app-40.png",
    "app-48.png",
    "app-64.png",
    "app-256.png",
];

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }
    let dir = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../assets/app/windows");
    let mut res = Vec::new();
    // A .res file opens with an empty entry.
    resource(&mut res, 0, 0, 0, &[]);
    let mut group = vec![0, 0, 1, 0];
    group.extend((IMAGES.len() as u16).to_le_bytes());
    for (id, name) in (1u16..).zip(IMAGES) {
        let path = dir.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let png = fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(&png[1..4], b"PNG", "{name} is not a PNG");
        // IHDR: width and height, big-endian, at 16; 256 is written as 0.
        let side = u32::from_be_bytes(png[16..20].try_into().unwrap());
        // GRPICONDIRENTRY: width, height, colors, reserved, planes, bits, size, id.
        group.extend([side as u8, side as u8, 0, 0]);
        group.extend(1u16.to_le_bytes());
        group.extend(32u16.to_le_bytes());
        group.extend((png.len() as u32).to_le_bytes());
        group.extend(id.to_le_bytes());
        resource(&mut res, RT_ICON, id, 0x1010, &png);
    }
    resource(&mut res, RT_GROUP_ICON, 1, 0x1030, &group);
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("shot.res");
    fs::write(&out, res).unwrap();
    println!("cargo:rustc-link-arg-bins={}", out.display());
}

/// Appends one resource with a numeric type and name, language neutral.
fn resource(res: &mut Vec<u8>, kind: u16, name: u16, flags: u16, data: &[u8]) {
    res.extend((data.len() as u32).to_le_bytes());
    res.extend(32u32.to_le_bytes());
    for id in [kind, name] {
        res.extend(0xFFFFu16.to_le_bytes());
        res.extend(id.to_le_bytes());
    }
    res.extend(0u32.to_le_bytes()); // data version
    res.extend(flags.to_le_bytes());
    res.extend(0u16.to_le_bytes()); // language
    res.extend(0u32.to_le_bytes()); // version
    res.extend(0u32.to_le_bytes()); // characteristics
    res.extend(data);
    res.resize(res.len().next_multiple_of(4), 0);
}
