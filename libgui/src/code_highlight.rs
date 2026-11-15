use alloc::vec::Vec;
use crate::gfx::Color;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CodeLanguage {
    Rust,
    C,
    Python,
    JavaScript,
    Shell,
    Json,
    TomlIni,
    HtmlCss,
    Markdown,
    Plain,
}

impl CodeLanguage {
    pub fn name(self) -> &'static str {
        match self {
            CodeLanguage::Rust => "Rust",
            CodeLanguage::C => "C / C++",
            CodeLanguage::Python => "Python",
            CodeLanguage::JavaScript => "JS / TS",
            CodeLanguage::Shell => "Shell",
            CodeLanguage::Json => "JSON",
            CodeLanguage::TomlIni => "TOML / INI",
            CodeLanguage::HtmlCss => "HTML / CSS",
            CodeLanguage::Markdown => "Markdown",
            CodeLanguage::Plain => "Plain Text",
        }
    }
}

pub fn detect_language(path: &str) -> CodeLanguage {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".rs") {
        CodeLanguage::Rust
    } else if lower.ends_with(".c") || lower.ends_with(".h") || lower.ends_with(".cpp") || lower.ends_with(".hpp") {
        CodeLanguage::C
    } else if lower.ends_with(".py") {
        CodeLanguage::Python
    } else if lower.ends_with(".js") || lower.ends_with(".ts") {
        CodeLanguage::JavaScript
    } else if lower.ends_with(".sh") || lower.ends_with(".sublimerc") || lower.ends_with(".zshrc") {
        CodeLanguage::Shell
    } else if lower.ends_with(".json") {
        CodeLanguage::Json
    } else if lower.ends_with(".toml") || lower.ends_with(".ini") || lower.ends_with(".app") {
        CodeLanguage::TomlIni
    } else if lower.ends_with(".html") || lower.ends_with(".css") {
        CodeLanguage::HtmlCss
    } else if lower.ends_with(".md") {
        CodeLanguage::Markdown
    } else {
        CodeLanguage::Plain
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    Normal,
    Keyword,
    Type,
    StringLit,
    Comment,
    Number,
    Operator,
    MacroOrAttr,
    Function,
    Tag,
}

#[derive(Clone, Copy, Debug)]
pub struct TokenSpan {
    pub start: usize,
    pub end: usize,
    pub kind: TokenKind,
}

pub fn token_color(kind: TokenKind, is_dark: bool) -> Color {
    if is_dark {
        match kind {
            TokenKind::Normal => Color::rgb(220, 224, 232),
            TokenKind::Keyword => Color::rgb(198, 120, 221),
            TokenKind::Type => Color::rgb(86, 182, 194),
            TokenKind::StringLit => Color::rgb(152, 195, 121),
            TokenKind::Comment => Color::rgb(112, 124, 144),
            TokenKind::Number => Color::rgb(209, 154, 102),
            TokenKind::Operator => Color::rgb(171, 178, 191),
            TokenKind::MacroOrAttr => Color::rgb(229, 192, 123),
            TokenKind::Function => Color::rgb(97, 175, 239),
            TokenKind::Tag => Color::rgb(224, 108, 117),
        }
    } else {
        match kind {
            TokenKind::Normal => Color::rgb(30, 34, 42),
            TokenKind::Keyword => Color::rgb(150, 25, 165),
            TokenKind::Type => Color::rgb(0, 125, 138),
            TokenKind::StringLit => Color::rgb(26, 126, 36),
            TokenKind::Comment => Color::rgb(112, 122, 136),
            TokenKind::Number => Color::rgb(176, 85, 0),
            TokenKind::Operator => Color::rgb(68, 76, 92),
            TokenKind::MacroOrAttr => Color::rgb(142, 92, 0),
            TokenKind::Function => Color::rgb(0, 92, 197),
            TokenKind::Tag => Color::rgb(185, 32, 48),
        }
    }
}

fn is_keyword(lang: CodeLanguage, word: &str) -> Option<TokenKind> {
    match lang {
        CodeLanguage::Rust => match word {
            "fn" | "let" | "mut" | "const" | "static" | "pub" | "struct" | "enum" | "impl"
            | "trait" | "for" | "in" | "while" | "loop" | "if" | "else" | "match" | "return"
            | "use" | "mod" | "crate" | "super" | "self" | "Self" | "where" | "as" | "unsafe"
            | "break" | "continue" | "type" | "ref" | "true" | "false" => Some(TokenKind::Keyword),
            "u8" | "u16" | "u32" | "u64" | "usize" | "i8" | "i16" | "i32" | "i64" | "isize"
            | "f32" | "f64" | "bool" | "char" | "str" | "String" | "Vec" | "Option" | "Result"
            | "Some" | "None" | "Ok" | "Err" => Some(TokenKind::Type),
            _ => None,
        },
        CodeLanguage::C => match word {
            "if" | "else" | "for" | "while" | "do" | "switch" | "case" | "default" | "break"
            | "continue" | "return" | "struct" | "enum" | "union" | "typedef" | "const"
            | "static" | "extern" | "inline" | "sizeof" | "true" | "false" | "NULL" => Some(TokenKind::Keyword),
            "void" | "char" | "short" | "int" | "long" | "float" | "double" | "unsigned"
            | "signed" | "uint8_t" | "uint16_t" | "uint32_t" | "uint64_t" | "int32_t"
            | "size_t" | "bool" => Some(TokenKind::Type),
            _ => None,
        },
        CodeLanguage::Python => match word {
            "def" | "class" | "if" | "elif" | "else" | "for" | "while" | "in" | "return"
            | "import" | "from" | "as" | "with" | "try" | "except" | "finally" | "pass"
            | "break" | "continue" | "and" | "or" | "not" | "is" | "None" | "True" | "False"
            | "lambda" | "yield" => Some(TokenKind::Keyword),
            "int" | "str" | "list" | "dict" | "tuple" | "set" | "bool" | "float" => Some(TokenKind::Type),
            _ => None,
        },
        CodeLanguage::JavaScript => match word {
            "function" | "const" | "let" | "var" | "if" | "else" | "for" | "while" | "return"
            | "switch" | "case" | "break" | "continue" | "class" | "extends" | "import"
            | "export" | "from" | "async" | "await" | "try" | "catch" | "new" | "this"
            | "true" | "false" | "null" | "undefined" | "interface" | "type" => Some(TokenKind::Keyword),
            "string" | "number" | "boolean" | "Array" | "Promise" | "Map" | "Set" => Some(TokenKind::Type),
            _ => None,
        },
        CodeLanguage::Shell => match word {
            "if" | "then" | "else" | "fi" | "for" | "in" | "do" | "done" | "while" | "case"
            | "esac" | "export" | "alias" | "cd" | "echo" | "print" | "list" | "view" | "cat"
            | "ls" | "grep" | "wc" | "mkdir" | "rm" | "exit" => Some(TokenKind::Keyword),
            _ => None,
        },
        CodeLanguage::Json | CodeLanguage::TomlIni => match word {
            "true" | "false" | "null" => Some(TokenKind::Keyword),
            _ => None,
        },
        _ => None,
    }
}

/// Tokenize a single line of code into non-overlapping highlighted spans.
pub fn tokenize_line(line: &str, lang: CodeLanguage, in_block_comment: &mut bool) -> Vec<TokenSpan> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut out = Vec::new();
    let mut i = 0usize;

    if lang == CodeLanguage::Plain {
        if len > 0 {
            out.push(TokenSpan { start: 0, end: len, kind: TokenKind::Normal });
        }
        return out;
    }

    if lang == CodeLanguage::Markdown {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            out.push(TokenSpan { start: 0, end: len, kind: TokenKind::Keyword });
            return out;
        }
        if trimmed.starts_with("```") {
            out.push(TokenSpan { start: 0, end: len, kind: TokenKind::MacroOrAttr });
            return out;
        }
        if trimmed.starts_with("> ") {
            out.push(TokenSpan { start: 0, end: len, kind: TokenKind::Comment });
            return out;
        }
    }

    while i < len {
        if *in_block_comment {
            let start = i;
            if let Some(end_rel) = line[i..].find("*/") {
                i += end_rel + 2;
                *in_block_comment = false;
            } else {
                i = len;
            }
            out.push(TokenSpan { start, end: i, kind: TokenKind::Comment });
            continue;
        }

        let b = bytes[i];
        // Line comments
        if ((lang == CodeLanguage::Rust || lang == CodeLanguage::C || lang == CodeLanguage::JavaScript)
            && line[i..].starts_with("//"))
            || ((lang == CodeLanguage::Python || lang == CodeLanguage::Shell || lang == CodeLanguage::TomlIni)
                && b == b'#')
        {
            out.push(TokenSpan { start: i, end: len, kind: TokenKind::Comment });
            break;
        }

        // Block comments
        if (lang == CodeLanguage::Rust || lang == CodeLanguage::C || lang == CodeLanguage::JavaScript || lang == CodeLanguage::HtmlCss)
            && line[i..].starts_with("/*")
        {
            *in_block_comment = true;
            let start = i;
            if let Some(end_rel) = line[i + 2..].find("*/") {
                i += 2 + end_rel + 2;
                *in_block_comment = false;
            } else {
                i = len;
            }
            out.push(TokenSpan { start, end: i, kind: TokenKind::Comment });
            continue;
        }

        // Preprocessor / Attributes / Section Headers
        if (lang == CodeLanguage::C && b == b'#')
            || (lang == CodeLanguage::Rust && line[i..].starts_with("#["))
            || (lang == CodeLanguage::TomlIni && b == b'[')
        {
            if lang == CodeLanguage::TomlIni {
                let start = i;
                while i < len && bytes[i] != b']' {
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
                out.push(TokenSpan { start, end: i, kind: TokenKind::MacroOrAttr });
                continue;
            } else if lang == CodeLanguage::C {
                out.push(TokenSpan { start: i, end: len, kind: TokenKind::MacroOrAttr });
                break;
            }
        }

        // Strings
        if b == b'"' || b == b'\'' {
            let quote = b;
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let kind = if lang == CodeLanguage::Json && line[i..].trim_start().starts_with(':') {
                TokenKind::Tag
            } else {
                TokenKind::StringLit
            };
            out.push(TokenSpan { start, end: i, kind });
            continue;
        }

        // HTML tags
        if lang == CodeLanguage::HtmlCss && b == b'<' {
            let start = i;
            i += 1;
            while i < len && bytes[i] != b'>' {
                i += 1;
            }
            if i < len {
                i += 1;
            }
            out.push(TokenSpan { start, end: i, kind: TokenKind::Tag });
            continue;
        }

        // Numbers
        if b.is_ascii_digit() {
            let start = i;
            i += 1;
            while i < len
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
            {
                i += 1;
            }
            out.push(TokenSpan { start, end: i, kind: TokenKind::Number });
            continue;
        }

        // Identifiers / Keywords / Functions / Macros
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            i += 1;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &line[start..i];
            let kind = if let Some(kw) = is_keyword(lang, word) {
                kw
            } else if lang == CodeLanguage::Rust && i < len && bytes[i] == b'!' {
                i += 1;
                TokenKind::MacroOrAttr
            } else if lang == CodeLanguage::TomlIni && line[i..].trim_start().starts_with('=') {
                TokenKind::Tag
            } else if i < len && bytes[i] == b'(' {
                TokenKind::Function
            } else if word.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false) {
                TokenKind::Type
            } else {
                TokenKind::Normal
            };
            out.push(TokenSpan { start, end: i, kind });
            continue;
        }

        // Operators / punctuation / whitespace
        let start = i;
        let is_op = matches!(b, b'=' | b'+' | b'-' | b'*' | b'/' | b'<' | b'>' | b'&' | b'|' | b'!' | b':' | b';');
        while i < len {
            let c = bytes[i];
            let c_op = matches!(c, b'=' | b'+' | b'-' | b'*' | b'/' | b'<' | b'>' | b'&' | b'|' | b'!' | b':' | b';');
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'"' || c == b'\'' || c == b'#' || (is_op != c_op) {
                break;
            }
            i += 1;
        }
        if i == start {
            i += 1;
            while i < len && !line.is_char_boundary(i) {
                i += 1;
            }
        }
        out.push(TokenSpan {
            start,
            end: i,
            kind: if is_op { TokenKind::Operator } else { TokenKind::Normal },
        });
    }

    out
}
