//! The app is a window program on Windows, so starting it opens no console beside the window.

/// Where the PE header's offset is kept in the DOS header.
#[cfg(windows)]
const PE_OFFSET_AT: usize = 0x3c;
/// Bytes from the PE signature to the optional header: the signature and the COFF header.
#[cfg(windows)]
const OPTIONAL_HEADER_FROM_PE: usize = 4 + 20;
/// Bytes from the optional header's start to its `Subsystem` field (PE32 and PE32+ alike).
#[cfg(windows)]
const SUBSYSTEM_IN_OPTIONAL_HEADER: usize = 68;
/// `IMAGE_SUBSYSTEM_WINDOWS_GUI`; a console program has 3.
#[cfg(windows)]
const WINDOWS_GUI: u16 = 2;

#[cfg(windows)]
#[test]
fn the_app_is_a_window_program() {
    let exe = std::fs::read(env!("CARGO_BIN_EXE_xml-atlas")).expect("read the app's exe");
    let at = |i: usize| u32::from_le_bytes(exe[i..i + 4].try_into().unwrap()) as usize;
    let pe = at(PE_OFFSET_AT);
    assert_eq!(&exe[pe..pe + 4], b"PE\0\0");
    let field = pe + OPTIONAL_HEADER_FROM_PE + SUBSYSTEM_IN_OPTIONAL_HEADER;
    let subsystem = u16::from_le_bytes([exe[field], exe[field + 1]]);
    assert_eq!(subsystem, WINDOWS_GUI);
}
