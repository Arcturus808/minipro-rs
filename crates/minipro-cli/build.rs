//! Build script: generate shell completion files and a man page at build time
//! into `$OUT_DIR/completions/` and `$OUT_DIR/man/` respectively.
//!
//! Packaging scripts can copy these files to the appropriate system locations:
//!   bash  →  /usr/share/bash-completion/completions/minipro
//!   zsh   →  /usr/share/zsh/site-functions/_minipro
//!   fish  →  /usr/share/fish/vendor_completions.d/minipro.fish
//!   ps    →  (PowerShell profile directory)
//!   man   →  /usr/share/man/man1/minipro.1.gz

use std::{env, fs, io::Write, path::PathBuf};

use clap::{ArgAction, CommandFactory, Parser};
use clap_complete::{generate_to, shells};
use clap_mangen::Man;

include!("src/cli.rs");

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let comp_dir = out_dir.join("completions");
    fs::create_dir_all(&comp_dir).expect("cannot create completions dir");

    let mut cmd = Cli::command();

    for shell in [
        shells::Shell::Bash,
        shells::Shell::Zsh,
        shells::Shell::Fish,
        shells::Shell::PowerShell,
    ] {
        generate_to(shell, &mut cmd, "minipro", &comp_dir)
            .unwrap_or_else(|e| panic!("failed to generate {shell:?} completions: {e}"));
    }

    // Man page
    let man_dir = out_dir.join("man");
    fs::create_dir_all(&man_dir).expect("cannot create man dir");
    let man_path = man_dir.join("minipro.1");
    let mut man_file = fs::File::create(&man_path).expect("cannot create minipro.1");

    let man = Man::new(Cli::command()).date("2026-05-17");
    man.render_title(&mut man_file)
        .expect("render_title failed");
    man.render_name_section(&mut man_file)
        .expect("render_name_section failed");
    man.render_synopsis_section(&mut man_file)
        .expect("render_synopsis_section failed");
    man.render_description_section(&mut man_file)
        .expect("render_description_section failed");
    man.render_options_section(&mut man_file)
        .expect("render_options_section failed");

    man_file
        .write_all(
            br#"
.SH NOTES ON FILE FORMATS
If the
.B \-\-format
option is not used when reading, the resulting file will be saved as a
raw binary file.
.P
If the ihex format is chosen and the data size is 64 kilobytes or smaller,
the file will be saved in ihex8 format.
If the data size exceeds 64 kilobytes, the ihex32 format is used.
.P
When writing chips, the format is automatically detected.
It is therefore not necessary to use the
.B \-\-format
option.

.SH NOTES ON MEMORY TYPES
The
.B \-\-page
option selects which memory region to operate on:
.TP
.B \-\-page 0
Code (flash/ROM) memory \(em the default.
.TP
.B \-\-page 1
Data (EEPROM) memory, where available.
.P
When
.B \-\-page
is omitted,
.B \-r
reads code memory and
.B \-w
writes code memory.
.P
Fuse and configuration bits are handled separately via
.B \-\-read\-fuses
and
.B \-\-write\-fuses .

.SH DATABASE FILES
.I minipro
reads chip definitions from two XML files:
.TP
.B infoic.xml
Chip database (MCUs, memory chips, etc.).
.TP
.B logicic.xml
Logic IC database (for logic IC testing with
.BR \-\-logic\-test ).
.P
File paths can be overridden explicitly with
.B \-\-infoic\-path
and
.B \-\-logicic\-path .
Otherwise, files are searched in the following order:
.RS
.IP 1. 4
Current working directory.
.IP 2. 4
Directory containing the
.I minipro
executable.
.IP 3. 4
.B MINIPRO_HOME
environment variable.
.IP 4. 4
.B %PROGRAMDATA%\eminipro\e
(Windows) or
.B /usr/share/minipro/
(Unix).
.RE

.SH UPDATING FIRMWARE
Firmware update files can be obtained from the manufacturer's website:
.nf
.B http://www.xgecu.com/en/
.fi
.TP
For the TL866A/CS, use the "update.dat" file.
.TP
For the TL866II+, use the "updateII.dat" file.
.TP
For the T48, use the "UpdateT48.dat" file.
.TP
For the T56, use the "updateT56.dat" file.
.TP
For the T76, use the "updateT76.dat" file.

.SH EXAMPLES
.TP
.B minipro \-p ATMEGA48 \-r atmega48.bin
Read the contents of an ATmega48 into a file.
.TP
.B minipro \-p ATMEGA48 \-w atmega48.bin
Write the contents of a file to an ATmega48.
.TP
.B minipro \-p \(dqAT29C256@DIP28\(dq \-w foobar.bin
Write to an AT29C256 EEPROM.
Remember to put quotes around device names containing the @ sign.
.TP
.B minipro \-p \(dqW25Q128@SOIC8\(dq \-r flash.bin
Read a 16 MiB SPI NOR flash chip.
.TP
.B minipro \-p 7404 \-\-logic\-test
Check whether a 74(LS/HC/...)04 hex NOT gate chip works correctly.
.TP
.B minipro \-l AT89
List all devices whose name contains "AT89".
.TP
.B minipro \-I
Show programmer model, hardware version, firmware version, and serial number.

.SH CAVEATS
The TL866A and TL866CS programmers appear to have a firmware bug such that
if not quite enough current is provided to them from a USB port, then the
programmer will fail to initialize itself or reset itself after an operation.
This problem seems to go hand\-in\-hand with newer USB 3.0 / xHCI ports and
can be avoided by using a powered hub.
.P
On Windows, the WinUSB driver must be installed via Zadig before the
programmer can be used.
Replug the device after driver installation.

.SH AUTHOR
.I minipro
was created by Valentin Dudouyt in 2014.
Many others have contributed code and bug reports.
Development of the original C project is coordinated by David Griffith.
.I minipro\-rs
is a Rust reimplementation by the minipro\-rs contributors.

.SH DISTRIBUTION
The canonical repository for
.I minipro\-rs
is at GitLab:
.nf
.B https://gitlab.com/arcturus8081/minipro\-rs/
.fi
.P
It is distributed under the GNU General Public License version 3 or
(at your option) any later version.
.nf
.B https://www.gnu.org/licenses/gpl\-3.0.en.html
.fi
"#,
        )
        .expect("failed to write extra man page sections");

    println!("cargo:rerun-if-changed=src/cli.rs");
    println!("cargo:rerun-if-changed=build.rs");
}
