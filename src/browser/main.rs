#[macro_use] extern crate html5ever_atoms;
extern crate html5ever;
extern crate orbclient;
extern crate orbfont;
extern crate tendril;

use std::{cmp, env, str};
use std::iter::repeat;
use std::default::Default;
use std::fs::File;
use std::io::{stderr, Read, Write};
use std::net::TcpStream;
use std::string::String;

use html5ever::parse_document;
use html5ever::rcdom::{Document, Doctype, Text, Comment, Element, RcDom, Handle};
use orbclient::{Color, Window, EventOption, K_ESC, K_DOWN, K_UP};
use orbfont::Font;
use tendril::TendrilSink;

#[derive(Clone, Debug)]
struct Url {
    scheme: String,
    host: String,
    port: u16,
    path: Vec<String>
}

impl Url {
    fn new(url: &str) -> Url {
        let (scheme, reference) = url.split_at(url.find(':').unwrap_or(0));

        let mut parts = reference.split('/').skip(2); //skip first two slashes
        let remote = parts.next().unwrap_or("");
        let mut remote_parts = remote.split(':');
        let host = remote_parts.next().unwrap_or("");
        let port = remote_parts.next().unwrap_or("").parse::<u16>().unwrap_or(80);

        let mut path = Vec::new();
        for part in parts {
            path.push(part.to_string());
        }

        Url {
            scheme: scheme.to_string(),
            host: host.to_string(),
            port: port,
            path: path
        }
    }
}

struct Block<'a> {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    color: Color,
    string: String,
    link: Option<String>,
    text: orbfont::Text<'a>
}

impl<'a> Block<'a> {
    fn contains(&self, m_x: i32, m_y: i32, offset: i32) -> bool {
        let x = self.x;
        let y = self.y - offset;

        m_x >= x && m_x < x + self.w && m_y >= y && m_y < y + self.h
    }

    fn draw(&self, window: &mut Window, offset: i32) {
        let x = self.x;
        let y = self.y - offset;
        if x + self.w > 0 && x < window.width() as i32 && y + self.h > 0 && y < window.height() as i32 {
            self.text.draw(window, x, y, self.color);
        }
    }
}

fn walk<'a>(handle: Handle, indent: usize, x: &mut i32, y: &mut i32, mut size: f32, mut bold: bool, mut color: Color, mut ignore: bool, whitespace: &mut bool, mut link: Option<String>, font: &'a Font, font_bold: &'a Font, blocks: &mut Vec<Block<'a>>) {
    let node = handle.borrow();

    let mut new_line = false;

    print!("{}", repeat(" ").take(indent).collect::<String>());
    match node.node {
        Document
            => {
                println!("#Document")
            },

        Doctype(ref name, ref public, ref system)
            => {
                println!("<!DOCTYPE {} \"{}\" \"{}\">", *name, *public, *system);
            },

        Text(ref text)
            => {
                let mut block_text = String::new();

                for c in text.chars() {
                    match c {
                        ' ' | '\n' | '\r' => if *whitespace {
                            // Ignore
                        } else {
                            // Set whitespace
                            *whitespace = true;
                            block_text.push(' ');
                        },
                        _ => {
                            if *whitespace {
                                *whitespace = false;
                            }
                            block_text.push(c);
                        }
                    }
                }

                if ! block_text.is_empty() {
                    if ignore {
                        println!("#text: ignored");
                    } else {
                        let trimmed_left = block_text.trim_left();
                        let left_margin = block_text.len() as i32 - trimmed_left.len() as i32;
                        let trimmed_right = trimmed_left.trim_right();
                        let right_margin = trimmed_left.len() as i32 - trimmed_right.len() as i32;

                        let escaped_text = escape_default(&trimmed_right);
                        println!("#text: block {} at {}, {}: '{}'", blocks.len(), *x, *y, escaped_text);

                        *x += left_margin * 8;

                        for (word_i, word) in trimmed_right.split(' ').enumerate() {
                            if word_i > 0 {
                                *x += 8;
                            }

                            let text = if bold {
                                font_bold.render(word, size)
                            } else {
                                font.render(word, size)
                            };

                            let w = text.width() as i32;
                            let h = text.height() as i32;

                            if *x + w >= 640 && *x > 0 {
                                *x = 0;
                                *y += size.ceil() as i32;
                            }

                            blocks.push(Block {
                                x: *x,
                                y: *y,
                                w: w,
                                h: h,
                                color: color,
                                string: word.to_string(),
                                link: link.clone(),
                                text: text
                            });

                            *x += w;
                        }

                        *x += right_margin * 8;
                    }
                } else {
                    println!("#text: empty");
                }
            },

        Comment(ref text)
            => {
                println!("<!-- {} -->", escape_default(text))
            },

        Element(ref name, _, ref attrs) => {
            assert!(name.ns == ns!(html));
            print!("<{}", name.local);
            for attr in attrs.iter() {
                assert!(attr.name.ns == ns!());
                print!(" {}=\"{}\"", attr.name.local, attr.value);
            }
            println!(">");

            match &*name.local {
                "a" => {
                    color = Color::rgb(0, 0, 255);
                    for attr in attrs.iter() {
                        match &*attr.name.local {
                            "href" => link = Some(attr.value.to_string()),
                            _ => ()
                        }
                    }
                },
                "b" => {
                    bold = true;
                },
                "br" => {
                    ignore = true;
                    new_line = true;
                },
                "div" => {
                    new_line = true;
                },
                "h1" => {
                    size = 32.0;
                    bold = true;
                    new_line = true;
                },
                "h2" => {
                    size = 24.0;
                    bold = true;
                    new_line = true;
                },
                "h3" => {
                    size = 18.0;
                    bold = true;
                    new_line = true;
                }
                "h4" => {
                    size = 16.0;
                    bold = true;
                    new_line = true;
                }
                "h5" => {
                    size = 14.0;
                    bold = true;
                    new_line = true;
                }
                "h6" => {
                    size = 10.0;
                    bold = true;
                    new_line = true;
                },
                "hr" => {
                    new_line = true;
                },
                "li" => {
                    new_line = true;
                },
                "p" => {
                    new_line = true;
                },

                "head" => ignore = true,
                "title" => ignore = true, //TODO: Grab title
                "link" => ignore = true,
                "meta" => ignore = true,
                "script" => ignore = true,
                "style" => ignore = true,
                _ => ()
            }
        }
    }

    for child in node.children.iter() {
        walk(child.clone(), indent + 4, x, y, size, bold, color, ignore, whitespace, link.clone(), font, font_bold, blocks);
    }

    if new_line {
        *whitespace = true;
        *x = 0;
        *y += size.ceil() as i32;
    }
}

// FIXME: Copy of str::escape_default from std, which is currently unstable
pub fn escape_default(s: &str) -> String {
    s.chars().flat_map(|c| c.escape_default()).collect()
}

fn read_blocks<'a, R: Read>(r: &mut R, font: &'a Font, font_bold: &'a Font) -> Vec<Block<'a>> {
    let mut blocks = vec![];

    let dom = parse_document(RcDom::default(), Default::default())
        .from_utf8()
        .read_from(r)
        .unwrap();

    let mut x = 0;
    let mut y = 0;
    let mut whitespace = false;
    walk(dom.document, 0, &mut x, &mut y, 16.0, false, Color::rgb(0, 0, 0), false, &mut whitespace, None, font, font_bold, &mut blocks);

    if !dom.errors.is_empty() {
        /*
        println!("\nParse errors:");
        for err in dom.errors.into_iter() {
            println!("    {}", err);
        }
        */
    }

    blocks
}

fn file_blocks<'a>(url: &Url, font: &'a Font, font_bold: &'a Font) -> Vec<Block<'a>> {
    let mut parts = url.path.iter();
    let mut path = parts.next().map_or(String::new(), |s| s.clone());
    for part in parts {
        path.push('/');
        path.push_str(part);
    }

    if let Ok(mut file) = File::open(&path) {
        read_blocks(&mut file, &font, &font_bold)
    } else {
        vec![]
    }
}

fn http_blocks<'a>(url: &Url, font: &'a Font, font_bold: &'a Font) -> Vec<Block<'a>> {
    let mut parts = url.path.iter();
    let mut path = parts.next().map_or(String::new(), |s| s.clone());
    for part in parts {
        path.push('/');
        path.push_str(part);
    }

    write!(stderr(), "* Connecting to {}:{}\n", url.host, url.port).unwrap();

    let mut stream = TcpStream::connect((url.host.as_str(), url.port)).unwrap();

    write!(stderr(), "* Requesting {}\n", path).unwrap();

    let request = format!("GET /{} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", path, url.host);
    stream.write(request.as_bytes()).unwrap();
    stream.flush().unwrap();

    write!(stderr(), "* Waiting for response\n").unwrap();

    let mut response = Vec::new();

    loop {
        let mut buf = [0; 65536];
        let count = stream.read(&mut buf).unwrap();
        if count == 0 {
            break;
        }
        response.extend_from_slice(&buf[.. count]);
    }

    write!(stderr(), "* Received {} bytes\n", response.len()).unwrap();

    let mut header_end = 0;
    while header_end < response.len() {
        if response[header_end..].starts_with(b"\r\n\r\n") {
            break;
        }
        header_end += 1;
    }

    for line in unsafe { str::from_utf8_unchecked(&response[..header_end]) }.lines() {
        write!(stderr(), "> {}\n", line).unwrap();
    }

    read_blocks(&mut &response[header_end + 4 ..], font, font_bold)
}

fn url_blocks<'a>(url: &Url, font: &'a Font, font_bold: &'a Font) -> Vec<Block<'a>> {
    if url.scheme == "http" {
        http_blocks(url, font, font_bold)
    } else if url.scheme == "file" || url.scheme.is_empty() {
        file_blocks(url, font, font_bold)
    } else {
        vec![]
    }
}

fn main_window(arg: &str, font: &Font, font_bold: &Font) {
    let mut url = Url::new(arg);

    let mut window = Window::new(-1, -1, 640, 480,  &format!("Browser ({})", arg)).unwrap();

    let mut blocks = url_blocks(&url, &font, &font_bold);

    let mut offset = 0;
    let mut max_offset = 0;
    for block in blocks.iter() {
        if block.y + block.h > max_offset {
            max_offset = block.y + block.h;
        }
    }

    let mut mouse_down = false;

    let mut redraw = true;
    loop {
        if redraw {
            redraw = false;

            window.set(Color::rgb(255, 255, 255));

            for block in blocks.iter() {
                block.draw(&mut window, offset);
            }

            window.sync();
        }

        for event in window.events() {
            match event.to_option() {
                EventOption::Key(key_event) => if key_event.pressed {
                    match key_event.scancode {
                        K_ESC => return,
                        K_UP => {
                            redraw = true;
                            offset = cmp::max(0, offset - 128);
                        },
                        K_DOWN => {
                            redraw = true;
                            offset = cmp::min(max_offset, offset + 128);
                        },
                        _ => ()
                    }
                },
                EventOption::Mouse(mouse_event) => if mouse_event.left_button {
                    mouse_down = true;
                } else if mouse_down {
                    mouse_down = false;

                    let mut link_opt = None;
                    for block in blocks.iter() {
                        if block.contains(mouse_event.x, mouse_event.y, offset) {
                            println!("Click {}", block.string);
                            if let Some(ref link) = block.link {
                                link_opt = Some(link.clone());
                                break;
                            }
                        }
                    }

                    if let Some(link) = link_opt {
                        if link.starts_with('#') {
                            println!("Find anchor {}", link);
                        } else {
                            if link.find(':').is_some() {
                                url = Url::new(&link);
                            } else if link.starts_with('/') {
                                url.path.clear();
                                for part in link[1..].split('/') {
                                    url.path.push(part.to_string());
                                }
                            } else {
                                url.path.push(link.clone());
                            };

                            println!("Navigate {}: {:#?}", link, url);

                            blocks = url_blocks(&url, &font, &font_bold);

                            offset = 0;
                            max_offset = 0;
                            for block in blocks.iter() {
                                if block.y + block.h > max_offset {
                                    max_offset = block.y + block.h;
                                }
                            }

                            redraw = true;
                        }
                    }
                },
                EventOption::Quit(_) => return,
                _ => ()
            }
        }
    }
}

fn main() {
    let err_window = |msg: &str| {
        let mut window = Window::new(-1, -1, 320, 32, "Browser").unwrap();

        window.set(Color::rgb(0, 0, 0));

        let mut x = 0;
        for c in msg.chars() {
            window.char(x, 0, c, Color::rgb(255, 255, 255));
            x += 8;
        }

        window.sync();

        loop {
            for event in window.events() {
                if let EventOption::Key(key_event) = event.to_option() {
                    if key_event.pressed && key_event.scancode == K_ESC {
                        return;
                    }
                }
                if let EventOption::Quit(_) = event.to_option() {
                    return;
                }
            }
        }
    };

    match env::args().nth(1) {
        Some(path) => match Font::find(None, None, None) {
            Ok(font) => match Font::find(None, None, Some("Bold")) {
                Ok(font_bold) => main_window(&path, &font, &font_bold),
                Err(err) => err_window(&format!("{}", err))
            },
            Err(err) => err_window(&format!("{}", err))
        },
        None => err_window("no file argument")
    }
}