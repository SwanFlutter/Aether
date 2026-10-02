// >>> AETHER-APP-PATCH build-provenance
//! Compile-time build provenance for the engine — Windows-Port von
//! `native/aether/aether/build.rs` im Android-Repo.
//!
//! ## Warum diese Datei existiert
//!
//! Der Engine-Banner druckt nur die UPSTREAM-Kernversion (`Aether v2.0.0`), und
//! die ist in jedem Patch-Stand identisch. Auf Android hat das eine ganze
//! Analyse-Runde gekostet: Fünf voneinander unabhängige Log-Zeilen bewiesen
//! hinterher, dass das getestete Binary den Fix unter Test überhaupt nicht
//! enthielt. Niemand konnte zwei Revisionen auseinanderhalten.
//!
//! Also stempelt die Engine ab jetzt auch auf dem Desktop den PATCH-STAND DER
//! APP zur Compile-Zeit ins Binary. `AETHER-BUILD-STAMP:` ist ein einziges
//! zusammenhängendes Literal, damit ein `findstr`/`grep -a` es in der
//! gestrippten `aether.exe` findet; CI prüft es im gepackten Payload, und die
//! App vergleicht ihren eigenen Stand zur Laufzeit gegen den, den die Engine
//! meldet.
//!
//! Netto: Die ersten Zeilen jedes künftigen Logs sagen genau, welcher Build ihn
//! erzeugt hat. Eine veraltete Engine lässt sich nicht mehr unbemerkt ausliefern
//! und nicht mehr versehentlich testen.

use std::path::{Path, PathBuf};

/// Läuft vom Crate-Verzeichnis aufwärts und sucht die `PATCHLEVEL`-Datei der
/// Repo-Wurzel.
///
/// Der Crate wird sowohl aus `native/aether/aether` im Baum als auch aus einer
/// von CI heruntergeladenen Kopie gebaut, die Tiefe ist also nicht fest.
/// `APP_PATCHLEVEL` aus der Umgebung gewinnt immer — das ist, was CI setzt.
fn discover_patchlevel() -> String {
    if let Ok(v) = std::env::var("APP_PATCHLEVEL") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            return v;
        }
    }

    let mut dir: PathBuf = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(".").to_path_buf());

    for _ in 0..6 {
        let candidate = dir.join("PATCHLEVEL");
        if let Ok(text) = std::fs::read_to_string(&candidate) {
            let v = text.trim().to_string();
            if !v.is_empty() {
                return v;
            }
        }
        if !dir.pop() {
            break;
        }
    }

    // Niemals stillschweigend eine Version erfinden. "unstamped" ist ein
    // lautes, greppbares Ergebnis, und die App behandelt es als harte Warnung.
    "unstamped".to_string()
}

fn main() {
    let patchlevel = discover_patchlevel();

    // Die Marke ist ein einziges zusammenhängendes Literal, damit `grep -a` auf
    // der gestrippten Binärdatei sie findet. Nicht umformatieren.
    println!("cargo:rustc-env=AETHER_APP_PATCHLEVEL={patchlevel}");
    println!("cargo:rustc-env=AETHER_BUILD_STAMP=AETHER-BUILD-STAMP:{patchlevel}");

    println!("cargo:rerun-if-env-changed=APP_PATCHLEVEL");
    println!("cargo:rerun-if-changed=../../../PATCHLEVEL");
    println!("cargo:rerun-if-changed=../../PATCHLEVEL");
    println!("cargo:rerun-if-changed=../PATCHLEVEL");
}
// <<< AETHER-APP-PATCH build-provenance
