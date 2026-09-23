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
| `crates/agent` | The model editing files: the harness (the loop), the editor agent, the four tools and the sandbox that keeps every path under the start-up directory. Knows nothing about terminals or HTTP; `docs/harness.md` is the design. |
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

1. `make release`, from `dev`, does the whole thing: it proposes the next
   version (patch, minor, major or one you type), writes it to `VERSION`,
   `Cargo.toml` and `Cargo.lock`, shows a summary and asks before touching
   anything. On a yes it commits the bump on `dev`, pushes it, merges `dev`
   into `main` with `--no-ff`, tags `vX.Y.Z` and pushes the branch and the
   tag, leaving you back on `dev`.
2. From `main` it is a promotion: the version is kept, and the tag is created
   and pushed if it was not there yet. From any other branch it refuses, as it
   does with a dirty tree, with no `origin`, or when `VERSION` and
   `Cargo.toml` disagree. If the merge collides only in `VERSION` it resolves
   it with the release version; any other conflict aborts the merge and leaves
   you back on `dev` with nothing published. `make set-version x.y.z` is still
   there to bump by hand.
3. The tag triggers the `release` workflow: it builds `moon` for every Linux,
   macOS and Windows target, writes `checksums.txt` and publishes the GitHub
   release. `install.sh`, `install.ps1` and the download tables in the README
   point at those assets.

The same archives can be built locally: `make package` for the host,
`make package-all` for every target (Linux and Windows through `cross`, which
needs Docker; macOS targets need a Mac), then `make checksums`. `make help`
lists the rest.
