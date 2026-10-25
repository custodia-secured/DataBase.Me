# @databaseme/sdk

The DataBase.Me plugin SDK. It is licensed under the **Apache License 2.0** (see [LICENSE](LICENSE)),
unlike the rest of the repository, so anyone, including commercial projects, can build plugins
without taking on the AGPL (ADR-0004).

**Status: reserved.** The plugin API, manifest format and sample plugins ship with milestone M10.
The design is fixed in [ADR-0005](../../docs/adr/0005-plugin-model.md): logic plugins are
WebAssembly modules run by Extism with capabilities the user grants explicitly, and UI plugins run
in sandboxed iframes that talk to the app only through a typed message channel.
