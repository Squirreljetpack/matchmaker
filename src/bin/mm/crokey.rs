/// Include the crokey demo to help users test keys
// MIT License

// Copyright (c) 2022 Canop

// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:

// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
use {
    crokey::*,
    crossterm::{
        event::{Event, read},
        style::Stylize,
        terminal,
    },
};

pub fn main() {
    let fmt = KeyCombinationFormat::default();
    println!("Type any key combination (remember that your terminal intercepts many ones)");
    loop {
        terminal::enable_raw_mode().unwrap();
        let e = read();
        terminal::disable_raw_mode().unwrap();
        match e {
            Ok(Event::Key(key_event)) => {
                let key_combination = key_event.into();
                let key = fmt.to_string(key_combination);
                match key_combination {
                    key!(ctrl - c) => {
                        println!("Arg! You savagely killed me with a {}", key.red());
                        break;
                    }
                    key!(ctrl - q) | key!(esc) => {
                        println!("You typed {} which gracefully quits", key.green());
                        break;
                    }
                    key!('?') | key!(shift - '?') => {
                        println!("{}", "Type Esc or Ctrl-q to exit".blue());
                    }
                    _ => {
                        println!("You typed {}", key.blue());
                    }
                }
            }
            e => {
                // any other event, for example a resize, we quit
                eprintln!("Quitting on {:?}", e);
                break;
            }
        }
    }
}
