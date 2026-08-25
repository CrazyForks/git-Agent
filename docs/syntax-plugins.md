# Syntax highlighting plugins

Git Agent keeps the syntax engine in the application and loads language definitions as data.
Plugins do not execute native code. A plugin is a directory below
`data/plugins/syntax/<plugin-id>` containing this layout:

```text
plugin-id/
├─ plugin.json
└─ syntaxes/
   └─ Language.sublime-syntax
```

The manifest format is versioned independently from the application:

```json
{
  "id": "example-language",
  "name": "Example Language",
  "version": "1.0.0",
  "api_version": 1,
  "syntaxes_dir": "syntaxes"
}
```

`syntaxes_dir` is optional and defaults to `syntaxes`. It must remain inside the plugin
directory. Plugins with invalid JSON, an incompatible API version, or syntax definitions that
cannot be loaded are ignored and reported in the diagnostic log. The syntax registry is built
once at process startup, so installing or updating a plugin currently requires restarting Git
Agent.

The installer includes the common syntax set curated by `two-face`/`bat`. External plugins are
intended for extra or organization-specific languages rather than duplicating the bundled set.
Themes stay under application control: syntax definitions emit TextMate/Sublime scopes and Git
Agent maps those scopes to the active light or dark palette.

## JSX and TSX

JSX and TSX remain language syntaxes, not React-only plugins. Git Agent can attach a framework
context (`react`, `vue`, `preact`, or `solid`) using, in priority order:

1. A per-file `@jsxImportSource` pragma.
2. The owning `tsconfig.json`, including `extends` and project references.
3. Imports in the source file.
4. The nearest `package.json`.

Framework detection is advisory and does not change the base JSX/TSX parser. Vue single-file
components use the bundled Vue syntax, including embedded template, script, and style regions.
