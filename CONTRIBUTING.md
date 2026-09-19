# Contributing to moon

Thanks for taking a look. moon is small on purpose, and contributions that
keep it that way are the most welcome: a bug fix with a test, a provider that
speaks a different API, a rough edge in the interface made smooth.

## Set up

You need Rust 1.88 or newer. Everything runs on the host: moon is a terminal
application, there is no Docker.

```sh
git clone https://github.com/towerforge/moon
cd moon
make check                  # fmt --check, clippy -D warnings, tests
make run ARGS="ask hello"   # run the CLI from source
make run                    # run the TUI from source
```

`make check` is what CI runs. If it is green, you are good.

## Where things are

| Crate | What it is |
|---|---|
| `crates/core` | The `Provider` trait, the domain types, configuration, XDG paths, JSONL sessions, files in the context. Knows nothing about terminals or HTTP. |
| `crates/providers/ollama` | Ollama over its native API. |
| `crates/providers/openai` | Any OpenAI-compatible chat completions API. |
| `crates/tui` | The interface, Elm style: `app/mod.rs` holds the state and `update`, each other module under `app/` is one `impl App` about one concern, `view/` paints. |
| `crates/cli` | The `moon` binary: wires providers, loads configuration, starts the TUI or a subcommand. |

Adding a provider is a new crate that implements `Provider` and
`ProviderFactory` from `moon-core`, plus one `registry.register(...)` line in
`crates/cli/src/main.rs`. Look at `crates/providers/openai` for the shape.

## Conventions

- Identifiers, comments and log messages in English. Every string the user
  sees is English too.
- No source file over 1000 lines. If one grows past that, split it by concern
  as `crates/tui/src/app/` does.
- Tests live next to the code (`#[cfg(test)]`), or in `tests.rs` files inside
  a module folder. Rendering is tested on ratatui's `TestBackend`; providers
  on `wiremock`.
- Commits follow [Conventional Commits](https://www.conventionalcommits.org):
  `feat(tui): …`, `fix(ollama): …`, `refactor(core): …`.
- Work happens on the `dev` branch. Open pull requests against it.

## Releasing

1. `make set-version x.y.z` writes the new version to `VERSION`, `Cargo.toml`
   and `Cargo.lock`. Review the diff and commit it on `dev`.
2. `make release` tags `v$VERSION` and pushes the tag. It refuses to run if
   `VERSION` and `Cargo.toml` disagree or the working tree is dirty.
3. The tag triggers the `release` workflow: it builds `moon` for every Linux,
   macOS and Windows target, writes `checksums.txt` and publishes the GitHub
   release. `install.sh`, `install.ps1` and the download tables in the README
   point at those assets.

The same archives can be built locally: `make package` for the host,
`make package-all` for every target (Linux and Windows through `cross`, which
needs Docker; macOS targets need a Mac), then `make checksums`. `make help`
lists the rest.
