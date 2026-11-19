# minipro-core

Core library for talking to XGecu/MiniPro TL866-family USB chip programmers
(TL866II+, TL866A/CS, T48, T56, T76): XML device-database lookup, model-specific
USB protocols, and read/write/erase/verify operations.

This is the engine behind the [`minipro`
CLI](https://gitlab.com/arcturus8081/minipro-rs) and its TUI/GUI frontends.

## Quick start

```rust,no_run
use minipro_core::{MiniproHandle, DatabasePaths, find_device};
use minipro_core::device::ProgrammerModel;
use minipro_core::operations::read_chip;

let db = DatabasePaths::resolve(None, None, None)?;
let device = std::sync::Arc::new(find_device(&db, "AT28C256", ProgrammerModel::Tl866iiPlus)?);
let mut handle = MiniproHandle::open()?;
handle.begin_transaction(device)?;
read_chip(&mut handle, std::path::Path::new("dump.bin"), 0, "auto", None)?;
handle.end_transaction()?;
# Ok::<(), minipro_core::error::MiniproError>(())
```

## Notes

- The chip database (`infoic.xml` / `logicic.xml` / `algorithm.xml`) is loaded
  at runtime — see `DatabasePaths::resolve` for the search order, and
  `xgpro_fetch` for downloading it from XGecu's official installer.
- Library diagnostics print to stderr via the `outln!` macro by default
  (upstream `minipro` behavior). Set `PRINT_TO_LOG` to route them through
  `log::info!` instead — required when your application owns the terminal.
- Async runtimes: wrap calls in `spawn_blocking` — the USB layer is
  synchronous.

## License

GPL-3.0-or-later, matching the upstream C `minipro` project this is derived
from.
