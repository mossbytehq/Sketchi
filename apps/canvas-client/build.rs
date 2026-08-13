#![allow(missing_docs)]

use std::{env, error::Error, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-env-changed=SKETCHI_EMBED_SERVER");

    let generated = PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is not set")?)
        .join("embedded_server.rs");
    let source = match env::var_os("SKETCHI_EMBED_SERVER") {
        Some(server) => {
            let server = PathBuf::from(server);
            if !server.is_file() {
                return Err(format!(
                    "SKETCHI_EMBED_SERVER does not point to a file: {}",
                    server.display()
                )
                .into());
            }
            println!("cargo:rerun-if-changed={}", server.display());
            let path = server.to_string_lossy().to_string();
            format!(concat!(
                "#[allow(missing_docs)]\n",
                "pub static EMBEDDED_SERVER: &[u8] = include_bytes!({path:?});\n"
            ))
        }
        None => "#[allow(missing_docs)]\npub static EMBEDDED_SERVER: &[u8] = &[];\n".to_owned(),
    };

    fs::write(generated, source)?;
    Ok(())
}
