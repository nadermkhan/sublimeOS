use crate::arch::x86_64::vga::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiCommand {
    None,
    SetColor { fg: Color, bg: Color },
    ClearScreen,
    ClearLine,
    CursorPosition { row: usize, col: usize },
    CursorUp(usize),
    CursorDown(usize),
    CursorForward(usize),
    CursorBack(usize),
    ShowCursor(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnsiState {
    Ground,
    Escape,
    Csi,
}

pub struct AnsiParser {
    state: AnsiState,
    params: [usize; 8],
    param_count: usize,
    current_param: usize,
    is_private: bool,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
}

impl AnsiParser {
    pub const fn new() -> Self {
        Self {
            state: AnsiState::Ground,
            params: [0; 8],
            param_count: 0,
            current_param: 0,
            is_private: false,
            fg: Color::LightGray,
            bg: Color::Black,
            bold: false,
        }
    }

    /// Process a single byte. Returns `Some(AnsiCommand)` if an ANSI sequence completed,
    /// or `None` if the byte was part of an escape sequence or a normal ground character.
    pub fn process_byte(&mut self, byte: u8) -> (bool, Option<AnsiCommand>) {
        match self.state {
            AnsiState::Ground => {
                if byte == 0x1B {
                    self.state = AnsiState::Escape;
                    (true, None) // Consumed by escape parser
                } else {
                    (false, None) // Normal character to be printed
                }
            }
            AnsiState::Escape => {
                if byte == b'[' {
                    self.state = AnsiState::Csi;
                    self.param_count = 0;
                    self.current_param = 0;
                    self.params = [0; 8];
                    self.is_private = false;
                    (true, None)
                } else {
                    // Unknown escape sequence, return to ground
                    self.state = AnsiState::Ground;
                    (true, None)
                }
            }
            AnsiState::Csi => {
                match byte {
                    b'?' => {
                        self.is_private = true;
                        (true, None)
                    }
                    b'0'..=b'9' => {
                        self.current_param = self.current_param * 10 + ((byte - b'0') as usize);
                        (true, None)
                    }
                    b';' => {
                        if self.param_count < self.params.len() {
                            self.params[self.param_count] = self.current_param;
                            self.param_count += 1;
                        }
                        self.current_param = 0;
                        (true, None)
                    }
                    // Final characters
                    b'm' => {
                        // SGR (Select Graphic Rendition) - Color and Styling
                        if self.param_count < self.params.len() {
                            self.params[self.param_count] = self.current_param;
                            self.param_count += 1;
                        }
                        self.state = AnsiState::Ground;
                        self.handle_sgr();
                        (true, Some(AnsiCommand::SetColor { fg: self.fg, bg: self.bg }))
                    }
                    b'J' => {
                        // Clear Screen
                        self.state = AnsiState::Ground;
                        (true, Some(AnsiCommand::ClearScreen))
                    }
                    b'K' => {
                        // Clear Line
                        self.state = AnsiState::Ground;
                        (true, Some(AnsiCommand::ClearLine))
                    }
                    b'H' | b'f' => {
                        // Cursor position: ESC [ <row> ; <col> H
                        if self.param_count < self.params.len() {
                            self.params[self.param_count] = self.current_param;
                            self.param_count += 1;
                        }
                        let row = if self.param_count > 0 && self.params[0] > 0 {
                            self.params[0] - 1
                        } else {
                            0
                        };
                        let col = if self.param_count > 1 && self.params[1] > 0 {
                            self.params[1] - 1
                        } else {
                            0
                        };
                        self.state = AnsiState::Ground;
                        (true, Some(AnsiCommand::CursorPosition { row, col }))
                    }
                    b'A' => {
                        let count = if self.current_param > 0 { self.current_param } else { 1 };
                        self.state = AnsiState::Ground;
                        (true, Some(AnsiCommand::CursorUp(count)))
                    }
                    b'B' => {
                        let count = if self.current_param > 0 { self.current_param } else { 1 };
                        self.state = AnsiState::Ground;
                        (true, Some(AnsiCommand::CursorDown(count)))
                    }
                    b'C' => {
                        let count = if self.current_param > 0 { self.current_param } else { 1 };
                        self.state = AnsiState::Ground;
                        (true, Some(AnsiCommand::CursorForward(count)))
                    }
                    b'D' => {
                        let count = if self.current_param > 0 { self.current_param } else { 1 };
                        self.state = AnsiState::Ground;
                        (true, Some(AnsiCommand::CursorBack(count)))
                    }
                    b'h' if self.is_private && self.current_param == 25 => {
                        self.state = AnsiState::Ground;
                        (true, Some(AnsiCommand::ShowCursor(true)))
                    }
                    b'l' if self.is_private && self.current_param == 25 => {
                        self.state = AnsiState::Ground;
                        (true, Some(AnsiCommand::ShowCursor(false)))
                    }
                    _ => {
                        self.state = AnsiState::Ground;
                        (true, None)
                    }
                }
            }
        }
    }

    fn handle_sgr(&mut self) {
        if self.param_count == 0 {
            // Default reset
            self.fg = Color::LightGray;
            self.bg = Color::Black;
            self.bold = false;
            return;
        }

        for i in 0..self.param_count {
            match self.params[i] {
                0 => {
                    self.fg = Color::LightGray;
                    self.bg = Color::Black;
                    self.bold = false;
                }
                1 => {
                    self.bold = true;
                    self.fg = to_bright(self.fg);
                }
                22 => {
                    self.bold = false;
                }
                // Standard Foreground Colors 30..37
                30 => self.fg = if self.bold { Color::DarkGray } else { Color::Black },
                31 => self.fg = if self.bold { Color::LightRed } else { Color::Red },
                32 => self.fg = if self.bold { Color::LightGreen } else { Color::Green },
                33 => self.fg = if self.bold { Color::Yellow } else { Color::Brown },
                34 => self.fg = if self.bold { Color::LightBlue } else { Color::Blue },
                35 => self.fg = if self.bold { Color::Pink } else { Color::Magenta },
                36 => self.fg = if self.bold { Color::LightCyan } else { Color::Cyan },
                37 => self.fg = if self.bold { Color::White } else { Color::LightGray },
                39 => self.fg = Color::LightGray,

                // High-Intensity Foreground Colors 90..97
                90 => self.fg = Color::DarkGray,
                91 => self.fg = Color::LightRed,
                92 => self.fg = Color::LightGreen,
                93 => self.fg = Color::Yellow,
                94 => self.fg = Color::LightBlue,
                95 => self.fg = Color::Pink,
                96 => self.fg = Color::LightCyan,
                97 => self.fg = Color::White,

                // Background Colors 40..47
                40 => self.bg = Color::Black,
                41 => self.bg = Color::Red,
                42 => self.bg = Color::Green,
                43 => self.bg = Color::Brown,
                44 => self.bg = Color::Blue,
                45 => self.bg = Color::Magenta,
                46 => self.bg = Color::Cyan,
                47 => self.bg = Color::LightGray,
                49 => self.bg = Color::Black,
                _ => {}
            }
        }
    }
}

fn to_bright(c: Color) -> Color {
    match c {
        Color::Black => Color::DarkGray,
        Color::Blue => Color::LightBlue,
        Color::Green => Color::LightGreen,
        Color::Cyan => Color::LightCyan,
        Color::Red => Color::LightRed,
        Color::Magenta => Color::Pink,
        Color::Brown => Color::Yellow,
        Color::LightGray => Color::White,
        other => other,
    }
}
