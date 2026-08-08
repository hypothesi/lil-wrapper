use crate::File;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LanguageKind {
    Plain,
    Markdown,
    Rst,
    Latex,
    Html,
    Source,
    Yaml,
}

#[derive(Clone, Copy)]
struct Language {
    name: &'static str,
    aliases: &'static [&'static str],
    paths: &'static [&'static str],
    kind: LanguageKind,
}

macro_rules! language {
    ($name:literal, [$($alias:literal),*], [$($path:literal),*], $kind:ident) => {
        Language { name: $name, aliases: &[$($alias),*], paths: &[$($path),*], kind: LanguageKind::$kind }
    };
}

static LANGUAGES: &[Language] = &[
    language!("AsciiDoc", ["asciidoc"], [".adoc", ".asciidoc"], Plain),
    language!("AutoHotkey", ["autohotkey", "ahk"], [".ahk"], Source),
    language!("Basic", ["basic", "vb"], [".vb"], Source),
    language!("Batch file", ["batch file", "bat"], [".bat"], Source),
    language!("Bikeshed", ["bikeshed"], [".bs"], Markdown),
    language!(
        "C/C++",
        ["c/c++", "c", "c++", "cpp"],
        [".c", ".cpp", ".h"],
        Source
    ),
    language!("C#", ["c#", "csharp"], [".cs"], Source),
    language!(
        "Clojure",
        ["clojure"],
        [".clj", ".cljs", ".cljc", ".cljx", ".edn"],
        Source
    ),
    language!("CMake", ["cmake"], ["cmakelists.txt"], Source),
    language!("CoffeeScript", ["coffeescript"], [".coffee"], Source),
    language!(
        "Common Lisp",
        ["common lisp", "commonlisp", "lisp"],
        [".lisp"],
        Source
    ),
    language!(
        "Configuration",
        ["configuration", "properties"],
        [".conf", ".gitconfig", ".pylintrc", "pylintrc"],
        Source
    ),
    language!("Crystal", ["crystal"], [".cr"], Source),
    language!(
        "CSS",
        ["css", "postcss"],
        [".css", ".pcss", ".postcss"],
        Source
    ),
    language!("D", ["d"], [".d"], Source),
    language!("Dart", ["dart"], [".dart"], Source),
    language!(
        "Dockerfile",
        ["dockerfile", "docker"],
        ["dockerfile"],
        Source
    ),
    language!("Elixir", ["elixir"], [".ex", ".exs"], Source),
    language!("Elm", ["elm"], [".elm"], Source),
    language!(
        "Emacs Lisp",
        ["emacs lisp", "elisp", "emacslisp"],
        [".el"],
        Source
    ),
    language!("F#", ["f#", "fsharp"], [".fs", ".fsx"], Source),
    language!("FIDL", ["fidl"], [".fidl"], Source),
    language!("Go", ["go"], [".go"], Source),
    language!(
        "Git commit",
        ["git commit", "git-commit"],
        ["tag_editmsg"],
        Markdown
    ),
    language!("GraphQL", ["graphql"], [".graphql", ".gql"], Source),
    language!("Groovy", ["groovy"], [".groovy"], Source),
    language!(
        "Handlebars",
        ["handlebars"],
        [".handlebars", ".hbs"],
        Source
    ),
    language!("Haskell", ["haskell"], [".hs"], Source),
    language!("HCL", ["hcl", "terraform"], [".hcl", ".tf"], Source),
    language!(
        "HTML",
        ["html", "erb", "htmlx", "svelte", "vue"],
        [".htm", ".html", ".svelte", ".vue"],
        Html
    ),
    language!("INI", ["ini"], [".ini"], Source),
    language!("J", ["j"], [".ijs"], Source),
    language!("Java", ["java"], [".java"], Source),
    language!(
        "JavaScript",
        ["javascript", "javascriptreact", "js"],
        [".js", ".jsx"],
        Source
    ),
    language!("Julia", ["julia"], [".jl"], Source),
    language!(
        "JSON",
        ["json", "json5", "jsonc"],
        [".json", ".json5", ".jsonc"],
        Source
    ),
    language!(
        "LaTeX",
        ["latex", "tex"],
        [".bbx", ".cbx", ".cls", ".sty", ".tex"],
        Latex
    ),
    language!("Lean", ["lean"], [".lean"], Source),
    language!("Less", ["less"], [".less"], Source),
    language!("Lua", ["lua"], [".lua"], Source),
    language!("Makefile", ["makefile", "make"], ["makefile"], Source),
    language!(
        "Markdown",
        ["markdown", "mdx"],
        [".md", ".mdx", ".rmd"],
        Markdown
    ),
    language!("MATLAB", ["matlab"], [], Source),
    language!("Objective-C", ["objective-c"], [".m", ".mm"], Source),
    language!("Octave", ["octave"], [], Source),
    language!("Pascal", ["pascal", "delphi"], [".pas"], Source),
    language!(
        "Perl",
        ["perl", "perl6"],
        [".p6", ".pl", ".pl6", ".pm", ".pm6"],
        Source
    ),
    language!("PHP", ["php"], [".php"], Source),
    language!(
        "PowerShell",
        ["powershell"],
        [".ps1", ".psd1", ".psm1"],
        Source
    ),
    language!("Prisma", ["prisma"], [".prisma"], Source),
    language!("Prolog", ["prolog"], [], Source),
    language!(
        "Protobuf",
        ["protobuf", "proto", "proto3"],
        [".proto"],
        Source
    ),
    language!("Pug", ["pug", "jade"], [".jade", ".pug"], Source),
    language!("PureScript", ["purescript"], [".purs"], Source),
    language!("Python", ["python"], [".py"], Source),
    language!("R", ["r"], [".r"], Source),
    language!(
        "reStructuredText",
        ["restructuredtext", "rst"],
        [".rst", ".rest"],
        Rst
    ),
    language!("Ruby", ["ruby"], [".rb"], Source),
    language!("Rust", ["rust"], [".rs"], Source),
    language!("SCSS", ["scss"], [".scss"], Source),
    language!("Scala", ["scala"], [".scala"], Source),
    language!(
        "Scheme",
        ["scheme"],
        [".scm", ".ss", ".sch", ".rkt"],
        Source
    ),
    language!("Shaderlab", ["shaderlab"], [".shader"], Source),
    language!(
        "Shell script",
        ["shell script", "shellscript"],
        [".sh"],
        Source
    ),
    language!(
        "SQL",
        ["sql", "postgres"],
        [".pgsql", ".psql", ".sql"],
        Source
    ),
    language!("Swift", ["swift"], [".swift"], Source),
    language!("Tcl", ["tcl"], [".tcl"], Source),
    language!("Textile", ["textile"], [".textile"], Markdown),
    language!("TOML", ["toml"], [".toml"], Source),
    language!(
        "TypeScript",
        ["typescript", "typescriptreact"],
        [".ts", ".tsx"],
        Source
    ),
    language!(
        "Verilog/SystemVerilog",
        ["verilog/systemverilog", "systemverilog", "verilog"],
        [".sv", ".svh", ".v", ".vh", ".vl"],
        Source
    ),
    language!("XAML", ["xaml"], [".xaml"], Html),
    language!("XML", ["xml", "xsl"], [".xml", ".xsl"], Html),
    language!("YAML", ["yaml"], [".yaml", ".yml"], Yaml),
];

fn language_for_file(file: &File) -> Option<&'static Language> {
    let id = file.language.to_ascii_lowercase();
    if !id.trim().is_empty() && id != "plaintext" {
        return LANGUAGES
            .iter()
            .find(|language| language.aliases.contains(&id.as_str()));
    }
    let file_name = file
        .path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    LANGUAGES.iter().find(|language| {
        language.paths.iter().any(|path| {
            path.strip_prefix('.')
                .map_or_else(|| file_name == *path, |_| file_name.ends_with(path))
        })
    })
}

#[must_use]
pub fn language_name_for_file(file: &File) -> Option<&'static str> {
    language_for_file(file).map(|language| language.name)
}

#[must_use]
pub fn languages() -> Vec<&'static str> {
    LANGUAGES.iter().map(|language| language.name).collect()
}

pub(crate) fn language_kind(file: &File) -> LanguageKind {
    language_for_file(file).map_or(LanguageKind::Plain, |language| language.kind)
}

pub(crate) fn canonical_language_name(file: &File) -> Option<&'static str> {
    language_for_file(file).map(|language| language.name)
}
