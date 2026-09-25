use super::*;
use crate::text_input::TextInput;
use crossterm::{
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind,
        KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::io::{self, stdout};

struct ScreenGuard;
impl ScreenGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
        Ok(Self)
    }
    fn pause(&self) -> Result<()> {
        disable_raw_mode()?;
        execute!(stdout(), DisableBracketedPaste, LeaveAlternateScreen)?;
        Ok(())
    }
    fn resume(&self) -> Result<()> {
        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
        Ok(())
    }
}
impl Drop for ScreenGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), DisableBracketedPaste, LeaveAlternateScreen);
    }
}

struct Player {
    _stream: OutputStream,
    handle: OutputStreamHandle,
    sink: Option<Sink>,
}
impl Player {
    fn new() -> Result<Self> {
        let (stream, handle) = OutputStream::try_default()?;
        Ok(Self {
            _stream: stream,
            handle,
            sink: None,
        })
    }
    fn play(&mut self, path: &Path) -> Result<()> {
        self.stop();
        let sink = Sink::try_new(&self.handle)?;
        sink.append(Decoder::new(BufReader::new(File::open(path)?))?);
        sink.play();
        self.sink = Some(sink);
        Ok(())
    }
    fn stop(&mut self) {
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
    }
}

#[derive(PartialEq)]
enum Mode {
    Menu,
    Speakers,
    Review,
    Edit,
}

#[derive(Clone, Copy, PartialEq)]
enum Language {
    English,
    Turkish,
}

impl Language {
    fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Turkish => "tr",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Turkish => "Türkçe",
        }
    }
    fn toggle(&mut self) {
        *self = match self {
            Self::English => Self::Turkish,
            Self::Turkish => Self::English,
        }
    }
}

struct App {
    root: PathBuf,
    mode: Mode,
    language: Language,
    menu_index: usize,
    speaker_index: usize,
    clip_index: usize,
    speaker: Option<PathBuf>,
    clips: Vec<String>,
    transcripts: BTreeMap<String, String>,
    reviews: BTreeMap<String, String>,
    input: TextInput,
    message: String,
    player: Option<Player>,
}

impl App {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            mode: Mode::Menu,
            language: Language::English,
            menu_index: 0,
            speaker_index: 0,
            clip_index: 0,
            speaker: None,
            clips: Vec::new(),
            transcripts: BTreeMap::new(),
            reviews: BTreeMap::new(),
            input: TextInput::default(),
            message: String::new(),
            player: None,
        }
    }
    fn speakers(&self) -> Vec<PathBuf> {
        speaker_dirs(&self.root)
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.join("output/wav").is_dir())
            .collect()
    }
    fn select_speaker(&mut self, speaker: PathBuf) -> Result<()> {
        self.player.as_mut().map(Player::stop);
        let output = speaker.join("output");
        self.transcripts = read_transcripts(&output.join("transcripts.tsv"))?;
        self.reviews = read_reviews(&output.join("reviews.tsv"))?;
        self.clips = fs::read_dir(output.join("wav"))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("wav"))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        self.clips.sort();
        self.clip_index = 0;
        self.speaker = Some(speaker);
        self.mode = Mode::Review;
        self.message = String::new();
        self.play_current();
        Ok(())
    }
    fn current(&self) -> Option<&str> {
        self.clips.get(self.clip_index).map(String::as_str)
    }
    fn save(&mut self) -> Result<()> {
        let out = self
            .speaker
            .as_ref()
            .context("speaker not selected")?
            .join("output");
        let name = self.current().context("clip not selected")?.to_owned();
        let (transcripts, reviews) = with_output_lock(&out, || {
            let mut transcripts = read_transcripts(&out.join("transcripts.tsv"))?;
            let mut reviews = read_reviews(&out.join("reviews.tsv"))?;
            if let Some(text) = self.transcripts.get(&name) {
                transcripts.insert(name.clone(), text.clone());
            }
            if let Some(status) = self.reviews.get(&name) {
                reviews.insert(name.clone(), status.clone());
            } else {
                reviews.remove(&name);
            }
            write_transcripts(&out.join("transcripts.tsv"), &transcripts)?;
            write_reviews(&out.join("reviews.tsv"), &reviews)?;
            finalize(&out)?;
            Ok((transcripts, reviews))
        })?;
        self.transcripts = transcripts;
        self.reviews = reviews;
        Ok(())
    }
    fn advance(&mut self) {
        if self.clip_index + 1 < self.clips.len() {
            self.clip_index += 1;
        }
    }
    fn play_current(&mut self) {
        let Some(name) = self.current().map(str::to_owned) else {
            return;
        };
        let path = self.speaker.as_ref().unwrap().join("output/wav").join(name);
        if self.player.is_none() {
            match Player::new() {
                Ok(p) => self.player = Some(p),
                Err(e) => {
                    self.message = format!(
                        "{}: {e}",
                        self.tr("Audio device unavailable", "Ses aygıtı açılamadı")
                    );
                    return;
                }
            }
        }
        match self.player.as_mut().unwrap().play(&path) {
            Ok(()) => {
                self.message = format!(
                    "{}: {}",
                    self.tr("Playing", "Çalıyor"),
                    path.file_name().unwrap().to_string_lossy()
                )
            }
            Err(e) => self.message = format!("{}: {e}", self.tr("Playback failed", "Çalınamadı")),
        }
    }
    fn set_status(&mut self, status: &str) {
        let Some(name) = self.current().map(str::to_owned) else {
            return;
        };
        if status == "approved"
            && self
                .transcripts
                .get(&name)
                .map(|s| clean_text(s).is_empty())
                .unwrap_or(true)
        {
            self.message = self
                .tr(
                    "Transcript is empty. Press E to edit.",
                    "Metin boş. Düzenlemek için E'ye bas.",
                )
                .into();
            return;
        }
        self.reviews.insert(name, status.into());
        match self.save() {
            Ok(()) => {
                self.advance();
                self.play_current();
            }
            Err(e) => self.message = format!("{}: {e:#}", self.tr("Save failed", "Kayıt hatası")),
        }
    }
    fn tr(&self, en: &'static str, tr: &'static str) -> &'static str {
        if self.language == Language::English {
            en
        } else {
            tr
        }
    }
}

pub fn run(root: PathBuf) -> Result<()> {
    let root = root.canonicalize().context("root directory missing")?;
    let guard = ScreenGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    let mut app = App::new(root);
    loop {
        terminal.draw(|frame| draw(frame, &app))?;
        let key = match event::read()? {
            Event::Key(key) => key,
            Event::Paste(text) if app.mode == Mode::Edit => {
                app.input.insert(&text);
                continue;
            }
            _ => continue,
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match app.mode {
            Mode::Menu => match key.code {
                KeyCode::Up => app.menu_index = app.menu_index.saturating_sub(1),
                KeyCode::Down => app.menu_index = (app.menu_index + 1).min(4),
                KeyCode::Left | KeyCode::Right if app.menu_index == 4 => app.language.toggle(),
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Enter => match app.menu_index {
                    0 | 1 => {
                        guard.pause()?;
                        if app.menu_index == 0 {
                            for s in speaker_dirs(&app.root)? {
                                if let Err(e) = prepare_speaker(&s, 1.5, 12.0) {
                                    eprintln!("{}: {e:#}", s.display());
                                }
                            }
                        } else if let Err(e) = transcribe_all(&app.root, app.language.code()) {
                            eprintln!("ASR error: {e:#}");
                        }
                        println!(
                            "{}",
                            app.tr("Press Enter to continue...", "Devam etmek için Enter...")
                        );
                        let mut line = String::new();
                        io::stdin().read_line(&mut line)?;
                        guard.resume()?;
                        terminal.clear()?;
                    }
                    2 => {
                        app.mode = Mode::Speakers;
                        app.speaker_index = 0;
                    }
                    3 => {
                        for s in speaker_dirs(&app.root)? {
                            let out = s.join("output");
                            if out.join("transcripts.tsv").is_file() {
                                finalize(&out)?;
                            }
                        }
                        app.message = app
                            .tr(
                                "Piper metadata updated.",
                                "Piper metadata dosyaları güncellendi.",
                            )
                            .into();
                    }
                    4 => {
                        app.language.toggle();
                        app.message =
                            format!("{}: {}", app.tr("Language", "Dil"), app.language.label());
                    }
                    _ => {}
                },
                _ => {}
            },
            Mode::Speakers => {
                let speakers = app.speakers();
                match key.code {
                    KeyCode::Up => app.speaker_index = app.speaker_index.saturating_sub(1),
                    KeyCode::Down => {
                        app.speaker_index =
                            (app.speaker_index + 1).min(speakers.len().saturating_sub(1))
                    }
                    KeyCode::Enter => {
                        if let Some(s) = speakers.get(app.speaker_index) {
                            if let Err(e) = app.select_speaker(s.clone()) {
                                app.message = format!("{}: {e:#}", app.tr("Error", "Hata"));
                            }
                        }
                    }
                    KeyCode::Esc | KeyCode::Char('q') => app.mode = Mode::Menu,
                    _ => {}
                }
            }
            Mode::Review => match key.code {
                KeyCode::Up | KeyCode::Left => {
                    app.clip_index = app.clip_index.saturating_sub(1);
                    app.play_current();
                }
                KeyCode::Down | KeyCode::Right => {
                    app.clip_index = (app.clip_index + 1).min(app.clips.len().saturating_sub(1));
                    app.play_current();
                }
                KeyCode::Char(' ') | KeyCode::Enter => app.play_current(),
                KeyCode::Char('a') | KeyCode::Char('A') => app.set_status("approved"),
                KeyCode::Char('r') | KeyCode::Char('R') => app.set_status("rejected"),
                KeyCode::Char('e') | KeyCode::Char('E') => {
                    if let Some(name) = app.current() {
                        app.input
                            .set(app.transcripts.get(name).cloned().unwrap_or_default());
                        app.mode = Mode::Edit;
                    }
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    if let Some(i) = app
                        .clips
                        .iter()
                        .enumerate()
                        .find(|(_, n)| !app.reviews.contains_key(*n))
                        .map(|(i, _)| i)
                    {
                        app.clip_index = i;
                        app.play_current();
                    }
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    app.player.as_mut().map(Player::stop);
                    app.mode = Mode::Speakers;
                }
                _ => {}
            },
            Mode::Edit => match key.code {
                KeyCode::Esc => app.mode = Mode::Review,
                KeyCode::Enter if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Some(name) = app.current().map(str::to_owned) {
                        app.transcripts
                            .insert(name.clone(), clean_text(app.input.value()));
                        app.reviews.remove(&name);
                        match app.save() {
                            Ok(()) => {
                                app.message = app
                                    .tr(
                                        "Transcript saved. Press A to approve.",
                                        "Metin kaydedildi. Onaylamak için A'ya bas.",
                                    )
                                    .into()
                            }
                            Err(e) => app.message = format!("{}: {e:#}", app.tr("Error", "Hata")),
                        }
                    }
                    app.mode = Mode::Review;
                }
                _ => app.input.handle(key),
            },
        }
    }
    Ok(())
}

fn draw(frame: &mut ratatui::Frame, app: &App) {
    let area = frame.area();
    if area.width < 48 || area.height < 12 {
        frame.render_widget(
            Paragraph::new(app.tr(
                "Make the terminal at least 48×12.",
                "Terminali en az 48×12 yapın.",
            ))
            .style(Style::default().fg(Color::Yellow)),
            area,
        );
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(4),
        ])
        .split(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " ◉ PIPER VOICE PREP ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {}  ", app.root.display()),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!("  {} ", app.language.label()),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .block(panel("", true))
        .alignment(Alignment::Left),
        chunks[0],
    );
    match app.mode {
        Mode::Menu => {
            let entries = if app.language == Language::English {
                [
                    "01  Clean and split audio",
                    "02  Transcribe with embedded Whisper",
                    "03  Review clips and transcripts",
                    "04  Build Piper metadata",
                    "05  Language: English  ⇄",
                ]
            } else {
                [
                    "01  Sesleri temizle ve böl",
                    "02  Yerleşik Whisper ile metin üret",
                    "03  Klipleri ve metinleri kontrol et",
                    "04  Piper metadata oluştur",
                    "05  Dil: Türkçe  ⇄",
                ]
            };
            let items = entries
                .iter()
                .map(|s| {
                    ListItem::new(Line::from(Span::styled(
                        *s,
                        Style::default().fg(Color::White),
                    )))
                })
                .collect::<Vec<_>>();
            let mut state = ListState::default().with_selected(Some(app.menu_index));
            frame.render_stateful_widget(
                List::new(items)
                    .block(panel(app.tr(" Choose an action ", " İşlem seç "), true))
                    .highlight_symbol(" ▶ ")
                    .highlight_style(selected_style()),
                chunks[1],
                &mut state,
            );
        }
        Mode::Speakers => {
            let speakers = app.speakers();
            let items = speakers
                .iter()
                .map(|p| {
                    ListItem::new(format!("  ◉  {}", p.file_name().unwrap().to_string_lossy()))
                })
                .collect::<Vec<_>>();
            let mut state = ListState::default().with_selected(Some(app.speaker_index));
            frame.render_stateful_widget(
                List::new(items)
                    .block(panel(app.tr(" Select a speaker ", " Konuşmacı seç "), true))
                    .highlight_style(selected_style()),
                chunks[1],
                &mut state,
            );
            if speakers.is_empty() {
                frame.render_widget(
                    Paragraph::new(app.tr(
                        "No prepared speakers. Run Clean and split audio first.",
                        "Hazırlanmış konuşmacı yok. Önce sesleri temizleyip bölün.",
                    ))
                    .style(Style::default().fg(Color::Yellow)),
                    inner(chunks[1]),
                );
            }
        }
        Mode::Review | Mode::Edit => {
            let body = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
                .split(chunks[1]);
            let items = app
                .clips
                .iter()
                .map(|name| {
                    let status = app
                        .reviews
                        .get(name)
                        .map(String::as_str)
                        .unwrap_or("pending");
                    let (mark, color) = match status {
                        "approved" => ("✓", Color::Green),
                        "rejected" => ("×", Color::Red),
                        _ => ("○", Color::DarkGray),
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(format!(" {mark} "), Style::default().fg(color)),
                        Span::raw(name.clone()),
                    ]))
                })
                .collect::<Vec<_>>();
            let mut state = ListState::default().with_selected(Some(app.clip_index));
            frame.render_stateful_widget(
                List::new(items)
                    .block(panel(
                        app.tr(" Clips ", " Klipler "),
                        app.mode == Mode::Review,
                    ))
                    .highlight_style(selected_style()),
                body[0],
                &mut state,
            );
            let right = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(3)])
                .split(body[1]);
            let approved = app.reviews.values().filter(|v| *v == "approved").count();
            let rejected = app.reviews.values().filter(|v| *v == "rejected").count();
            let progress = if app.clips.is_empty() {
                0.0
            } else {
                (approved + rejected) as f64 / app.clips.len() as f64
            };
            frame.render_widget(
                Gauge::default()
                    .block(panel(
                        &format!(
                            " {} / {}  •  ✓ {}  × {} ",
                            app.clip_index.saturating_add(1).min(app.clips.len()),
                            app.clips.len(),
                            approved,
                            rejected
                        ),
                        false,
                    ))
                    .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
                    .ratio(progress.min(1.0))
                    .label(format!("{:.0}%", progress * 100.0)),
                right[0],
            );
            let name = app.current().unwrap_or("");
            if app.mode == Mode::Edit {
                let edit = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(3), Constraint::Min(3)])
                    .split(right[1]);
                let input_area = inner(edit[0]);
                let (line, cursor_x) = app.input.view(input_area.width as usize);
                frame.render_widget(
                    Paragraph::new(line)
                        .block(panel(app.tr(" Edit transcript ", " Metni düzenle "), true)),
                    edit[0],
                );
                frame.render_widget(
                    Paragraph::new(app.input.value())
                        .style(Style::default().fg(Color::Gray))
                        .wrap(Wrap { trim: false })
                        .block(panel(app.tr(" Full text ", " Tam metin "), false)),
                    edit[1],
                );
                frame.set_cursor_position((input_area.x + cursor_x, input_area.y));
            } else {
                let text = app.transcripts.get(name).map(String::as_str).unwrap_or("");
                frame.render_widget(
                    Paragraph::new(if text.is_empty() {
                        app.tr("No transcript yet", "Henüz metin yok")
                    } else {
                        text
                    })
                    .style(Style::default().fg(if text.is_empty() {
                        Color::DarkGray
                    } else {
                        Color::White
                    }))
                    .wrap(Wrap { trim: false })
                    .block(panel(app.tr(" Transcript ", " Metin "), false)),
                    right[1],
                );
            }
        }
    }
    let hint = match app.mode {
        Mode::Menu => app.tr(
            "↑↓ Navigate  •  Enter Select  •  ←→ Language  •  Q Quit",
            "↑↓ Gezin  •  Enter Seç  •  ←→ Dil  •  Q Çık",
        ),
        Mode::Speakers => app.tr(
            "↑↓ Select speaker  •  Enter Review  •  Esc Back",
            "↑↓ Konuşmacı seç  •  Enter Kontrol  •  Esc Geri",
        ),
        Mode::Review => app.tr(
            "←→ Clip  •  Space Play  •  A Approve  •  R Reject  •  E Edit  •  N Next pending",
            "←→ Klip  •  Boşluk Dinle  •  A Onayla  •  R Reddet  •  E Düzenle  •  N Bekleyen",
        ),
        Mode::Edit => app.tr(
            "←→ Move  •  Shift Select  •  Ctrl+A All  •  Home/End  •  Enter Save  •  Esc Cancel",
            "←→ İmleç  •  Shift Seç  •  Ctrl+A Tümü  •  Home/End  •  Enter Kaydet  •  Esc İptal",
        ),
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(hint, Style::default().fg(Color::Cyan))),
            Line::from(Span::styled(
                if app.message.is_empty() {
                    ""
                } else {
                    &app.message
                },
                Style::default().fg(Color::Yellow),
            )),
        ])
        .block(panel(app.tr(" Help / Status ", " Yardım / Durum "), false)),
        chunks[2],
    );
}

fn panel(title: &str, focused: bool) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused {
            Color::Cyan
        } else {
            Color::DarkGray
        }))
        .title(Span::styled(
            title,
            Style::default()
                .fg(if focused { Color::Cyan } else { Color::Gray })
                .add_modifier(Modifier::BOLD),
        ))
}

fn selected_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

fn inner(area: Rect) -> Rect {
    Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}
