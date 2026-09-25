use super::*;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::io::{self, stdout};

struct ScreenGuard;
impl ScreenGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
    fn pause(&self) -> Result<()> {
        disable_raw_mode()?;
        execute!(stdout(), LeaveAlternateScreen)?;
        Ok(())
    }
    fn resume(&self) -> Result<()> {
        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen)?;
        Ok(())
    }
}
impl Drop for ScreenGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
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

struct App {
    root: PathBuf,
    mode: Mode,
    menu_index: usize,
    speaker_index: usize,
    clip_index: usize,
    speaker: Option<PathBuf>,
    clips: Vec<String>,
    transcripts: BTreeMap<String, String>,
    reviews: BTreeMap<String, String>,
    input: String,
    message: String,
    player: Option<Player>,
}

impl App {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            mode: Mode::Menu,
            menu_index: 0,
            speaker_index: 0,
            clip_index: 0,
            speaker: None,
            clips: Vec::new(),
            transcripts: BTreeMap::new(),
            reviews: BTreeMap::new(),
            input: String::new(),
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
        self.message = "Boşluk: dinle | A: onayla | R: reddet | E: düzenle".into();
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
                    self.message = format!("Ses aygıtı açılamadı: {e}");
                    return;
                }
            }
        }
        match self.player.as_mut().unwrap().play(&path) {
            Ok(()) => {
                self.message = format!("Çalıyor: {}", path.file_name().unwrap().to_string_lossy())
            }
            Err(e) => self.message = format!("Çalınamadı: {e}"),
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
            self.message = "Metin boş. Önce E ile düzenle.".into();
            return;
        }
        self.reviews.insert(name, status.into());
        match self.save() {
            Ok(()) => {
                self.advance();
                self.play_current();
            }
            Err(e) => self.message = format!("Kayıt hatası: {e:#}"),
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
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match app.mode {
            Mode::Menu => match key.code {
                KeyCode::Up => app.menu_index = app.menu_index.saturating_sub(1),
                KeyCode::Down => app.menu_index = (app.menu_index + 1).min(3),
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
                        } else if let Err(e) = transcribe_all(&app.root, "tr") {
                            eprintln!("ASR error: {e:#}");
                        }
                        println!("Devam etmek için Enter...");
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
                        app.message = "Piper metadata dosyaları güncellendi.".into();
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
                                app.message = format!("Hata: {e:#}");
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
                        app.input = app.transcripts.get(name).cloned().unwrap_or_default();
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
                KeyCode::Enter => {
                    if let Some(name) = app.current().map(str::to_owned) {
                        app.transcripts.insert(name.clone(), clean_text(&app.input));
                        app.reviews.remove(&name);
                        match app.save() {
                            Ok(()) => app.message = "Metin kaydedildi; onay için A.".into(),
                            Err(e) => app.message = format!("Hata: {e:#}"),
                        }
                    }
                    app.mode = Mode::Review;
                }
                KeyCode::Backspace => {
                    app.input.pop();
                }
                KeyCode::Char(c) => app.input.push(c),
                _ => {}
            },
        }
    }
    Ok(())
}

fn draw(frame: &mut ratatui::Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);
    frame.render_widget(
        Paragraph::new(format!("Piper Voice Prep  |  {}", app.root.display())).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Ses veri hazırlama"),
        ),
        chunks[0],
    );
    match app.mode {
        Mode::Menu => {
            let entries = [
                "Sesleri temizle ve böl",
                "Otomatik metin üret (yerleşik Whisper)",
                "Kontrol: dinle / düzenle / onayla",
                "Piper metadata oluştur",
            ];
            let items = entries
                .iter()
                .map(|s| ListItem::new(*s))
                .collect::<Vec<_>>();
            let mut state = ListState::default().with_selected(Some(app.menu_index));
            frame.render_stateful_widget(
                List::new(items)
                    .block(Block::default().borders(Borders::ALL).title("İşlem seç"))
                    .highlight_style(
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                chunks[1],
                &mut state,
            );
        }
        Mode::Speakers => {
            let items = app
                .speakers()
                .iter()
                .map(|p| ListItem::new(p.file_name().unwrap().to_string_lossy().to_string()))
                .collect::<Vec<_>>();
            let mut state = ListState::default().with_selected(Some(app.speaker_index));
            frame.render_stateful_widget(
                List::new(items)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Konuşmacı seç"),
                    )
                    .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan)),
                chunks[1],
                &mut state,
            );
        }
        Mode::Review | Mode::Edit => {
            let body = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
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
                    let mark = match status {
                        "approved" => "✓",
                        "rejected" => "×",
                        _ => "·",
                    };
                    ListItem::new(format!("{mark} {name}"))
                })
                .collect::<Vec<_>>();
            let mut state = ListState::default().with_selected(Some(app.clip_index));
            frame.render_stateful_widget(
                List::new(items)
                    .block(Block::default().borders(Borders::ALL).title("Klipler"))
                    .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan)),
                body[0],
                &mut state,
            );
            let name = app.current().unwrap_or("");
            let text = if app.mode == Mode::Edit {
                &app.input
            } else {
                app.transcripts.get(name).map(String::as_str).unwrap_or("")
            };
            let title = if app.mode == Mode::Edit {
                "Metni düzenle: Enter kaydet, Esc iptal"
            } else {
                "Metin: Boşluk çal, A onayla, R reddet, E düzenle, N bekleyen"
            };
            frame.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .block(Block::default().borders(Borders::ALL).title(title)),
                body[1],
            );
        }
    }
    let hint = if app.message.is_empty() {
        "Yön tuşları + Enter | Esc/Q geri"
    } else {
        &app.message
    };
    frame.render_widget(
        Paragraph::new(hint).block(Block::default().borders(Borders::ALL).title("Durum")),
        chunks[2],
    );
}
