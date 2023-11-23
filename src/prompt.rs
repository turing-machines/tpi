// Copyright 2023 Turing Machines
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::io::{stdout, Write};

use anyhow::{bail, Result};
use crossterm::cursor::MoveToColumn;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::style::Print;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType};
use crossterm::{execute, queue};

struct Prompt {
    msg: &'static str,
    password: bool,
    input: String,
    cursor_idx: usize,
}

impl Prompt {
    fn new(msg: &'static str, password: bool) -> Self {
        Self {
            msg,
            password,
            input: String::new(),
            cursor_idx: 0,
        }
    }

    fn read(&mut self) -> Result<String> {
        enable_raw_mode()?;

        let res = self.read_loop();

        disable_raw_mode()?;

        match res {
            Ok(()) => Ok(self.input.clone()),
            Err(e) => bail!("failed to get terminal event: {e}"),
        }
    }

    fn read_loop(&mut self) -> Result<()> {
        loop {
            self.print()?;

            let cont = match event::read()? {
                Event::Key(key) => self.handle_key(key)?,
                _ => true,
            };

            if !cont {
                break;
            }
        }

        Ok(())
    }

    fn print(&self) -> Result<()> {
        queue!(
            stdout(),
            MoveToColumn(0),
            Clear(ClearType::CurrentLine),
            Print(format!("{}: ", self.msg)),
        )?;

        if !self.password {
            let column = self.msg.len() + self.cursor_idx + 2;
            let column = u16::try_from(column).unwrap_or(0);

            queue!(stdout(), Print(&self.input), MoveToColumn(column))?;
        }

        stdout().flush()?;

        Ok(())
    }

    fn handle_key(&mut self, key: event::KeyEvent) -> Result<bool> {
        let interrupt = key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c');

        if interrupt || key.code == KeyCode::Enter {
            execute!(stdout(), Print("\n\r"))?;
            return Ok(false);
        }

        match key.code {
            KeyCode::Char(c) => {
                self.input.insert(self.cursor_idx, c);
                self.cursor_idx += 1;
            }
            KeyCode::Delete => {
                if !self.input.is_empty() {
                    self.delete();
                }
            }
            KeyCode::Backspace => {
                if !self.input.is_empty() {
                    self.left();
                    self.delete();
                }
            }
            KeyCode::Left => self.left(),
            KeyCode::Right => {
                if self.cursor_idx < self.input.len() - 1 {
                    self.cursor_idx += 1;
                }
            }
            _ => {}
        }

        Ok(true)
    }

    fn left(&mut self) {
        if self.cursor_idx > 0 {
            self.cursor_idx -= 1;
        }
    }

    fn delete(&mut self) {
        if self.cursor_idx < self.input.len() {
            self.input.remove(self.cursor_idx);
        }
    }
}

pub fn simple(msg: &'static str) -> Result<String> {
    Prompt::new(msg, false).read()
}

pub fn password(msg: &'static str) -> Result<String> {
    Prompt::new(msg, true).read()
}
