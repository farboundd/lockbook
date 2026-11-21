use super::axis::{Unit, YAxis};
use super::history::{DAY, History};
use super::queries::{self, Cohort, Retention, SeriesPoint};
use super::source::RefreshProgress;
use super::{Result, cache_sync_time, format_bytes, read_cache, source};
use eframe::egui::{
    self, Align2, Color32, Context, FontId, Frame, Rect, RichText, Stroke, Ui, Vec2,
};
use egui_plot::{
    AxisHints, GridMark, HoverPosition, Line as PlotLine, Plot, PlotBounds, Points, VLine,
};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use time::macros::format_description;
use workspace_rs::theme::icons::Icon;
use workspace_rs::widgets::Button;

const DATE: &[time::format_description::BorrowedFormatItem<'static>] =
    format_description!("[year]-[month]-[day]");
const TIME: &[time::format_description::BorrowedFormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute] UTC");
const CHART_MIN_WIDTH: f32 = 480.0;
const CHART_GAP: f32 = 12.0;
const STALE_AFTER_MS: i64 = 60 * 60 * 1000;
const LOGO_TURN_SECONDS: f32 = 1.2;
const LOGO_PAUSE_SECONDS: f32 = 4.8;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Overview,
    Retention,
    Accounts,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Job {
    Open,
    Refresh,
    Cached,
}

enum Update {
    Progress(RefreshProgress),
    Loaded(Result<Option<History>>, Job, Transfer),
    Analysis((i64, i64), Arc<History>, Analysis),
}

#[derive(Default)]
struct Transfer {
    synced_at: Option<i64>,
    bytes: Option<u64>,
}

enum DataSource {
    Production(Transfer),
    Local(String),
}

struct Analysis {
    overview: Vec<SeriesPoint>,
    retention: Retention,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sort {
    Storage,
    Egress,
    Activity,
    Age,
    Name,
}

pub struct Metrics {
    logo: egui::TextureHandle,
    directory: PathBuf,
    remote: String,
    path: String,
    history: Option<Arc<History>>,
    tab: Tab,
    range: Option<(i64, i64)>,
    preset_days: Option<i64>,
    draft_start: String,
    draft_end: String,
    range_error: Option<String>,
    at: Option<i64>,
    sequence_text: String,
    exact: Option<u64>,
    data_source: Option<DataSource>,
    source_error: Option<String>,
    job: Job,
    progress: Option<String>,
    segments: Option<(usize, usize)>,
    loading_started: Instant,
    query: Option<Analysis>,
    sort: Sort,
    search: String,
    selected_account: Option<usize>,
    sender: mpsc::Sender<Update>,
    updates: mpsc::Receiver<Update>,
    busy: bool,
}

impl Metrics {
    pub fn new(
        directory: PathBuf, remote: String, logs: Option<PathBuf>, logo: egui::TextureHandle,
        ctx: &Context,
    ) -> Self {
        let (sender, updates) = mpsc::channel();
        let mut app = Self {
            logo,
            directory,
            remote,
            path: String::new(),
            history: None,
            tab: Tab::Overview,
            range: None,
            preset_days: Some(30),
            draft_start: String::new(),
            draft_end: String::new(),
            range_error: None,
            at: None,
            sequence_text: String::new(),
            exact: None,
            data_source: None,
            source_error: None,
            job: Job::Cached,
            progress: None,
            segments: None,
            loading_started: Instant::now(),
            query: None,
            sort: Sort::Storage,
            search: String::new(),
            selected_account: None,
            sender,
            updates,
            busy: false,
        };
        let job = if let Some(path) = logs {
            app.path = path.to_string_lossy().into_owned();
            Job::Open
        } else {
            Job::Cached
        };
        app.load(job, ctx);
        app
    }

    fn load(&mut self, job: Job, ctx: &Context) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.job = job;
        self.segments = None;
        self.loading_started = Instant::now();
        self.progress = Some(
            match job {
                Job::Open => "Opening local logs…",
                Job::Cached => "Opening the local cache…",
                Job::Refresh => "Connecting to production…",
            }
            .into(),
        );
        self.source_error = None;
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        let directory = self.directory.clone();
        let remote = self.remote.clone();
        let path = PathBuf::from(&self.path);
        std::thread::spawn(move || {
            let mut transfer = Transfer::default();
            let progress = |event| {
                let _ = sender.send(Update::Progress(event));
                ctx.request_repaint();
            };
            let result = match job {
                Job::Open => source::local(&path, &progress).map(Some),
                Job::Cached => read_cache(&directory),
                Job::Refresh => {
                    source::refresh(&directory, &remote, &progress).map(|(history, bytes)| {
                        transfer.bytes = Some(bytes);
                        Some(history)
                    })
                }
            };
            if job != Job::Open && matches!(&result, Ok(Some(_))) {
                transfer.synced_at = cache_sync_time(&directory);
            }
            let _ = sender.send(Update::Loaded(result, job, transfer));
            ctx.request_repaint();
        });
    }

    fn updates(&mut self, ctx: &Context) {
        while let Ok(update) = self.updates.try_recv() {
            match update {
                Update::Progress(RefreshProgress::Message(progress)) => {
                    self.progress = Some(progress);
                }
                Update::Progress(RefreshProgress::Segments { complete, total }) => {
                    self.segments = Some((complete, total));
                    if complete == total {
                        self.progress = None;
                    }
                }
                Update::Analysis(range, history, analysis) => {
                    if self.range == Some(range)
                        && self
                            .history
                            .as_ref()
                            .is_some_and(|current| Arc::ptr_eq(current, &history))
                    {
                        self.query = Some(analysis);
                    }
                }
                Update::Loaded(result, job, transfer) => {
                    self.busy = false;
                    self.progress = None;
                    self.segments = None;
                    match result {
                        Ok(Some(history)) => {
                            let coverage = history
                                .anchors
                                .first()
                                .zip(history.anchors.last())
                                .map(|(&(start, _), &(end, _))| (start, end));
                            self.data_source = Some(match job {
                                Job::Open => DataSource::Local(self.path.clone()),
                                Job::Cached | Job::Refresh => DataSource::Production(transfer),
                            });
                            self.history = Some(Arc::new(history));
                            self.selected_account = None;
                            self.exact = None;
                            self.query = None;
                            if let Some((first, last)) = coverage {
                                if job != Job::Refresh {
                                    self.preset_days = Some(30);
                                }
                                let (start, end) = match self.preset_days {
                                    Some(days) => (preset_start(days, last).max(first), last),
                                    None => self
                                        .range
                                        .filter(|(start, end)| *start >= first && *end <= last)
                                        .unwrap_or(((last - 30 * DAY).max(first), last)),
                                };
                                self.set_range(start, end, ctx);
                            } else {
                                self.range = None;
                                self.at = None;
                            }
                        }
                        Ok(None) if job == Job::Cached => self.load(Job::Refresh, ctx),
                        Err(_) if job == Job::Cached => self.load(Job::Refresh, ctx),
                        Ok(None) => {}
                        Err(error) => self.source_error = Some(error.to_string()),
                    }
                }
            }
        }
    }

    fn set_range(&mut self, start: i64, end: i64, ctx: &Context) {
        self.range = Some((start, end));
        self.draft_start = date(start);
        self.draft_end = date(end);
        self.at = Some(end);
        self.exact = None;
        self.range_error = None;
        self.query = None;
        let Some(history) = self.history.clone() else { return };
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let overview = queries::overview(&history, start, end);
            let retention = queries::retention(&history, start, end);
            let _ = sender.send(Update::Analysis(
                (start, end),
                history,
                Analysis { overview, retention },
            ));
            ctx.request_repaint();
        });
    }

    fn selected(&self, history: &History) -> Option<(u64, Option<i64>)> {
        if let Some(seq) = self.exact {
            return Some((seq, history.timestamp_at(seq)));
        }
        if let Some(time) = self.at {
            return history.sequence_at(time).map(|seq| (seq, Some(time)));
        }
        Some((history.latest, history.timestamp_at(history.latest)))
    }

    fn controls(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.vertical_centered(|ui| {
            floating_frame(ui).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.scope(|ui| {
                        ui.set_max_width(244.0);
                        re_ui::TabBar::new(ui)
                            .selectable_value(&mut self.tab, Tab::Overview, "Overview")
                            .selectable_value(&mut self.tab, Tab::Retention, "Retention")
                            .selectable_value(&mut self.tab, Tab::Accounts, "Accounts");
                    });
                    ui.separator();
                    let label = self.range.map_or_else(|| "Date range".into(), |range| {
                        let now = current_time();
                        ctx.request_repaint_after(Duration::from_millis(
                            (queries::day(now) + DAY - now) as u64,
                        ));
                        range_label(range, now)
                    });
                    let range_button = Button::default().text(&label).show(ui);
                    egui::Popup::menu(&range_button).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                        let coverage = self.history.as_ref().and_then(|history| {
                            history.anchors.first().zip(history.anchors.last()).map(|(&(first, _), &(last, _))| (first, last))
                        });
                        if let Some((first, last)) = coverage {
                            ui.horizontal(|ui| {
                                for (days, label) in [(0, "Today"), (7, "7 days"), (14, "14 days"), (30, "30 days")] {
                                    if ui.button(label).clicked() {
                                        self.preset_days = Some(days);
                                        let start = preset_start(days, last);
                                        self.set_range(start.max(first), last, &ctx);
                                        ui.close();
                                    }
                                }
                            });
                            ui.separator();
                            ui.label("Custom range (UTC)");
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut self.draft_start).desired_width(112.0).hint_text("Start date"));
                                ui.add(egui::TextEdit::singleline(&mut self.draft_end).desired_width(112.0).hint_text("End date"));
                                if ui.button("Apply").clicked() {
                                    match (parse_date(&self.draft_start), parse_date(&self.draft_end)) {
                                        (Some(start), Some(end)) if start <= end && end + DAY > first && start <= last => {
                                            self.preset_days = None;
                                            self.set_range(start.max(first), (end + DAY - 1).min(last), &ctx);
                                            ui.close();
                                        }
                                        _ => self.range_error = Some("Enter dates within the imported coverage (YYYY-MM-DD).".into()),
                                    }
                                }
                            });
                            if let Some(error) = &self.range_error { ui.label(error); }
                        } else {
                            ui.label("Import logs with dated events to select a range.");
                        }
                    });
                });
            });
        });
    }

    fn source_button(&mut self, ctx: &Context) -> f32 {
        egui::Area::new(egui::Id::unique("metrics source"))
            .anchor(Align2::RIGHT_BOTTOM, [-20.0, -18.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                floating_frame(ui).inner_margin(5.0).show(ui, |ui| {
                    let source = Button::default()
                        .icon(&Icon::SYNC.size(14.0))
                        .text("Source")
                        .show(ui);
                    egui::Popup::menu(&source)
                        .align(egui::RectAlign::TOP_END)
                        .gap(8.0)
                        .frame(floating_frame(ui))
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            ui.set_max_width(360.0);
                            self.source_panel(ui, ctx);
                        });
                });
            })
            .response
            .rect
            .height()
    }

    fn source_panel(&mut self, ui: &mut Ui, ctx: &Context) {
        match &self.data_source {
            Some(DataSource::Production(transfer)) => {
                if let Some(synced_at) = transfer.synced_at {
                    let age = current_time().saturating_sub(synced_at);
                    let label = if self
                        .history
                        .as_ref()
                        .is_some_and(|history| history.partial_tail)
                    {
                        "Production data · incomplete"
                    } else if age >= STALE_AFTER_MS {
                        "Production data · stale"
                    } else {
                        ctx.request_repaint_after(Duration::from_millis(
                            (STALE_AFTER_MS - age) as u64,
                        ));
                        "Production data · fresh"
                    };
                    ui.strong(label);
                    ui.weak(format!("Last synced {}", timestamp(synced_at)));
                } else {
                    ui.strong("Production cache");
                    ui.weak("Sync time unavailable");
                }
                if let Some(bytes) = transfer.bytes {
                    ui.weak(if bytes == 0 {
                        "Already current · 0 log bytes downloaded".into()
                    } else {
                        format!("{} of log data downloaded", format_bytes(bytes))
                    });
                }
            }
            Some(DataSource::Local(path)) => {
                ui.strong("Local copy");
                ui.weak(path);
            }
            None => {}
        }
        if ui
            .add_enabled(!self.busy, egui::Button::new("Sync to latest"))
            .clicked()
        {
            self.load(Job::Refresh, ctx);
        }
        if self.busy {
            self.segment_progress(ui, 260.0);
            if let Some(progress) = &self.progress {
                ui.label(progress);
            }
        }
        if let Some(error) = &self.source_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if let Some(history) = &self.history {
            ui.weak(format!("Sequences {}–{}", history.oldest, history.latest));
            if let Some((first, last)) = history.anchors.first().zip(history.anchors.last()) {
                ui.weak(format!("{} – {}", timestamp(first.0), timestamp(last.0)));
            }
            if !history.gaps.is_empty() {
                ui.weak(format!("{} gaps in coverage", history.gaps.len()));
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.path)
                    .desired_width(215.0)
                    .hint_text("Local ServerV6 directory"),
            );
            if ui
                .add_enabled(
                    !self.busy && !self.path.is_empty(),
                    egui::Button::new("Open local logs"),
                )
                .clicked()
            {
                self.load(Job::Open, ctx);
            }
        });
    }

    fn segment_progress(&self, ui: &mut Ui, width: f32) -> bool {
        let Some((complete, total)) = self.segments.filter(|&(_, total)| total != 0) else {
            return false;
        };
        let (track, _) = ui.allocate_exact_size(Vec2::new(width, 3.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(track, 1.5, ui.visuals().widgets.noninteractive.bg_stroke.color);
        if complete > 0 {
            let filled = Rect::from_min_size(
                track.min,
                Vec2::new(track.width() * complete as f32 / total as f32, track.height()),
            );
            ui.painter()
                .rect_filled(filled, 1.5, ui.visuals().selection.bg_fill);
        }
        ui.add_space(3.0);
        let left = total - complete;
        ui.label(match left {
            0 => "Segment retrieval done".to_owned(),
            1 => "1 segment left".to_owned(),
            _ => format!("{left} segments left"),
        });
        true
    }

    fn onboarding(&mut self, ui: &mut Ui) {
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, ui.visuals().panel_fill);
        let ctx = ui.ctx().clone();
        let turn = if self.busy && ui.style().animation_time > 0.0 {
            let elapsed = self.loading_started.elapsed().as_secs_f32();
            let phase = (elapsed + LOGO_PAUSE_SECONDS - LOGO_TURN_SECONDS)
                % (LOGO_PAUSE_SECONDS + LOGO_TURN_SECONDS);
            let turn = ((phase - LOGO_PAUSE_SECONDS) / LOGO_TURN_SECONDS).clamp(0.0, 1.0);
            if turn > 0.0 {
                ctx.request_repaint_after(Duration::from_millis(16));
            } else {
                ctx.request_repaint_after(Duration::from_secs_f32(LOGO_PAUSE_SECONDS - phase));
            }
            turn
        } else {
            0.0
        };
        egui::Area::new(egui::Id::unique("metrics onboarding"))
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(&ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 48.0).clamp(200.0, 430.0));
                ui.vertical_centered(|ui| {
                    let size = 82.0;
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::splat(124.0), egui::Sense::hover());
                    let center = rect.center();
                    let blue = Color32::from_rgb(31, 122, 220);
                    ui.painter()
                        .circle_filled(center, 54.0, blue.gamma_multiply(0.10));
                    ui.painter().circle_stroke(
                        center,
                        53.0,
                        Stroke::new(1.0, blue.gamma_multiply(0.26)),
                    );
                    let logo = egui::Image::from_texture((self.logo.id(), Vec2::splat(size)));
                    let logo_rect = Rect::from_center_size(center, Vec2::splat(size));
                    let angle = logo_angle(turn);
                    if turn > 0.0 {
                        for (frames_ago, alpha) in [(4.0, 12), (3.0, 20), (2.0, 32), (1.0, 48)] {
                            let earlier = (turn - frames_ago * 0.012 / LOGO_TURN_SECONDS).max(0.0);
                            let trail = logo_angle(earlier);
                            if angle - trail > 0.015 {
                                logo.clone()
                                    .rotate(trail, Vec2::splat(0.5))
                                    .tint(Color32::from_white_alpha(alpha))
                                    .paint_at(ui, logo_rect);
                            }
                        }
                    }
                    logo.rotate(angle, Vec2::splat(0.5)).paint_at(ui, logo_rect);
                    ui.add_space(16.0);
                    ui.label(
                        RichText::new("Lockbook Metrics Explorer")
                            .size(32.0)
                            .strong(),
                    );
                    ui.add_space(12.0);
                    if self.busy {
                        if !self.segment_progress(ui, 300.0) {
                            ui.add(egui::Spinner::new().size(23.0));
                        }
                        if let Some(progress) = &self.progress {
                            ui.weak(progress);
                        }
                    } else if let Some(error) = &self.source_error {
                        let title = if self.job == Job::Open {
                            "Couldn’t open local logs"
                        } else {
                            "Couldn’t sync production logs"
                        };
                        ui.label(title);
                        ui.colored_label(ui.visuals().error_fg_color, error);
                        if self.job == Job::Refresh {
                            ui.weak("Check your gcloud login and production SSH access.");
                        }
                        ui.add_space(10.0);
                        if ui.button("Retry").clicked() {
                            self.load(self.job, &ctx);
                        }
                    }
                });
            });
    }

    fn state_control(&mut self, ui: &mut Ui, history: &History) {
        ui.horizontal_wrapped(|ui| {
            let title = match self.selected(history) {
                Some((seq, Some(time))) if self.exact.is_none() => {
                    format!("State at {} · estimated · sequence {seq}", timestamp(time))
                }
                Some((seq, _)) => format!("State at sequence {seq}"),
                None => "State unavailable for this date".into(),
            };
            let button = Button::default().text(&title).show(ui);
            egui::Popup::menu(&button)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    ui.label("Exact state by sequence");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.sequence_text)
                                .desired_width(110.0)
                                .hint_text("Sequence"),
                        );
                        if ui.button("Go").clicked() {
                            if let Ok(seq) = self.sequence_text.parse::<u64>() {
                                if history.sequence_covered(seq) {
                                    self.exact = Some(seq);
                                    self.at = history.timestamp_at(seq);
                                    ui.close();
                                }
                            }
                        }
                    });
                    if !self.sequence_text.is_empty()
                        && !self
                            .sequence_text
                            .parse::<u64>()
                            .is_ok_and(|seq| history.sequence_covered(seq))
                    {
                        ui.label("Sequence outside continuous imported coverage.");
                    }
                    if ui.button("End of range").clicked() {
                        self.exact = None;
                        self.at = self.range.map(|(_, end)| end);
                        ui.close();
                    }
                });
            if self.busy {
                ui.spinner();
            }
        });
    }

    fn overview(&mut self, ui: &mut Ui, history: &History) {
        if let Some((seq, _)) = self.selected(history) {
            if let Some(state) = history.checkpoint(seq) {
                let storage = if state.unknown_storage == 0 {
                    format_bytes(state.storage)
                } else {
                    "–".into()
                };
                surface(ui).show(ui, |ui| {
                    ui.columns(3, |columns| {
                        for (column, (label, value)) in columns.iter_mut().zip([
                            ("Users", state.users.to_string()),
                            ("Metadata entries", state.files.to_string()),
                            ("Document storage", storage),
                        ]) {
                            column.weak(label);
                            column.label(bold_text(value, 24.0));
                        }
                    });
                    if state.unknown_storage != 0 {
                        ui.weak("Storage unavailable");
                    }
                });
                ui.add_space(12.0);
            }
        }
        let Some(analysis) = &self.query else { return };
        let range = self.range.unwrap();
        let fields: [(&str, Unit, &[Line]); 9] = [
            ("Users", Unit::Count, &[("Total users", |p| p.users)]),
            ("Signups · trailing 7 days", Unit::Count, &[("Signups", |p| p.signups)]),
            (
                "Active users",
                Unit::Count,
                &[
                    ("24 hours", |p| p.active[0]),
                    ("48 hours", |p| p.active[1]),
                    ("7 days", |p| p.active[2]),
                    ("30 days", |p| p.active[3]),
                ],
            ),
            ("DAU / MAU", Unit::Percent, &[("Stickiness", |p| p.stickiness)]),
            ("Sharing", Unit::Count, &[("Users", |p| p.sharing)]),
            (
                "Premium",
                Unit::Count,
                &[
                    ("Total", |p| p.premium[0]),
                    ("Stripe", |p| p.premium[1]),
                    ("Google Play", |p| p.premium[2]),
                    ("App Store", |p| p.premium[3]),
                ],
            ),
            ("Metadata entries", Unit::Count, &[("Entries", |p| p.files)]),
            ("Document storage", Unit::Bytes, &[("Storage", |p| p.storage)]),
            ("Server egress · trailing 30 days", Unit::Bytes, &[("Egress", |p| p.egress)]),
        ];
        let charts = fields.map(|(title, unit, lines)| Chart {
            title,
            unit,
            series: lines
                .iter()
                .map(|(label, field)| {
                    (
                        *label,
                        analysis
                            .overview
                            .iter()
                            .map(|point| (point.time, field(point)))
                            .collect(),
                    )
                })
                .collect(),
        });
        if let Some(time) = chart_grid(ui, range, self.at, &charts) {
            self.at = Some(time);
            self.exact = None;
        }
    }

    fn retention(&mut self, ui: &mut Ui) {
        let Some(analysis) = &self.query else { return };
        let range = self.range.unwrap();
        surface(ui).show(ui, |ui| {
            ui.columns(4, |columns| {
                for (index, (ui, state)) in columns
                    .iter_mut()
                    .zip(analysis.retention.headline)
                    .enumerate()
                {
                    let label = if index == 0 {
                        if date(range.1) == date(current_time()) {
                            "Today · Day 0".to_owned()
                        } else {
                            format!("Day 0 · {}", date(range.1))
                        }
                    } else {
                        format!("Day {}", queries::RETENTION[index])
                    };
                    ui.weak(label);
                    match state {
                        Cohort::Complete(count) | Cohort::Partial(count) => {
                            let value = count
                                .percent()
                                .map_or_else(|| "–".into(), |value| format!("{value:.1}%"));
                            ui.label(bold_text(value, 24.0));
                            ui.weak(format!(
                                "{} of {} accounts{}",
                                count.returned,
                                count.eligible,
                                if matches!(state, Cohort::Partial(_)) {
                                    " · partial day"
                                } else {
                                    ""
                                }
                            ));
                            if index != 0 && analysis.retention.excluded[index] != 0 {
                                ui.weak(format!(
                                    "{} cohorts missing coverage",
                                    analysis.retention.excluded[index]
                                ));
                            }
                        }
                        Cohort::Pending => {
                            ui.label(bold_text("–", 24.0));
                            ui.weak("Not yet observable");
                        }
                        Cohort::Missing => {
                            ui.label(bold_text("–", 24.0));
                            ui.weak("Missing coverage");
                        }
                    }
                }
            });
        });
        ui.add_space(12.0);
        let series: Vec<_> = ["Day 0", "Day 7", "Day 14", "Day 30"]
            .into_iter()
            .enumerate()
            .map(|(index, label)| {
                (
                    label,
                    analysis
                        .retention
                        .cohorts
                        .iter()
                        .map(|(day, values)| ((*day).max(range.0), values[index].rate()))
                        .collect(),
                )
            })
            .collect();
        let chart = Chart { title: "Retention by signup date", unit: Unit::Percent, series };
        if let Some(time) = chart_grid(ui, range, self.at, &[chart]) {
            self.at = Some(time);
            self.exact = None;
        }
    }

    fn accounts(&mut self, ui: &mut Ui, history: &History) {
        let Some((seq, time)) = self.selected(history) else { return };
        if !history.sequence_covered(seq) {
            ui.label("Account state is unavailable in this gap.");
            return;
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .desired_width(245.0)
                    .hint_text("Search accounts"),
            );
            if self.selected_account.is_some() && ui.button("Back to accounts").clicked() {
                self.selected_account = None;
            }
        });
        if let Some(index) = self.selected_account {
            let account = &history.accounts[index];
            ui.label(bold_text(account.display_name().as_ref(), 22.0));
            if let Some(created) = account.created {
                ui.weak(format!("Created {}", date(created)));
            }
            if let Some(range) = self.range {
                let times = queries::sample_times(range.0, range.1);
                let [storage, egress] = [
                    ("Storage", (|s: &super::history::AccountState| s.storage) as fn(_) -> _),
                    ("Cumulative egress", |s| Some(s.egress)),
                ]
                .map(|(label, getter)| Chart {
                    title: label,
                    unit: Unit::Bytes,
                    series: vec![(
                        label,
                        times
                            .iter()
                            .map(|&time| {
                                (
                                    time,
                                    history
                                        .sequence_at(time)
                                        .filter(|seq| history.sequence_covered(*seq))
                                        .and_then(|seq| account.at(seq))
                                        .and_then(getter)
                                        .map(|value| value as f64),
                                )
                            })
                            .collect(),
                    )],
                });
                let hours = times
                    .iter()
                    .map(|&time| {
                        let start = (time - DAY).max(range.0);
                        (
                            time,
                            history
                                .covered(start, time)
                                .then(|| account.active_hours(start, time)),
                        )
                    })
                    .collect();
                let activity = Chart {
                    title: "Active hours · trailing day",
                    unit: Unit::Hours,
                    series: vec![("Hours", hours)],
                };
                if let Some(time) = chart_grid(ui, range, self.at, &[storage, egress, activity]) {
                    self.at = Some(time);
                    self.exact = None;
                }
            }
            return;
        }
        let search = self.search.to_lowercase();
        let egress_from = time.and_then(|time| {
            history
                .covered(time - 30 * DAY, time)
                .then(|| history.sequence_at(time - 30 * DAY))
                .flatten()
        });
        let activity_window = time
            .zip(self.range)
            .filter(|(end, (start, _))| history.covered(*start, *end));
        let mut rows: Vec<_> = history
            .accounts
            .iter()
            .enumerate()
            .filter_map(|(index, account)| {
                let state = account.at(seq)?;
                if !state.live {
                    return None;
                }
                let name = account.display_name();
                if !name.to_lowercase().contains(&search) {
                    return None;
                }
                let age = account
                    .created
                    .zip(time)
                    .map(|(created, time)| (time - created).max(0) / DAY);
                let egress = egress_from.and_then(|from| {
                    state
                        .egress
                        .checked_sub(account.at(from).map_or(0, |prior| prior.egress))
                });
                let active =
                    activity_window.map(|(end, (start, _))| account.active_hours(start, end));
                Some((index, state, age, egress, active, name))
            })
            .collect();
        rows.sort_by(|a, b| {
            match self.sort {
                Sort::Storage => b.1.storage.cmp(&a.1.storage),
                Sort::Egress => b.3.cmp(&a.3),
                Sort::Activity => b.4.partial_cmp(&a.4).unwrap_or(std::cmp::Ordering::Equal),
                Sort::Age => b.2.cmp(&a.2),
                Sort::Name => a.5.cmp(&b.5),
            }
            .then_with(|| a.5.cmp(&b.5))
        });
        let width = (ui.available_width() - 4.0 * ui.spacing().item_spacing.x) / 5.0;
        ui.horizontal(|ui| {
            for (sort, label) in [
                (Sort::Name, "Account"),
                (Sort::Age, "Age"),
                (Sort::Activity, "Active hours"),
                (Sort::Storage, "Storage"),
                (Sort::Egress, "30d egress"),
            ] {
                if ui
                    .add_sized([width, 26.0], egui::Button::selectable(self.sort == sort, label))
                    .clicked()
                {
                    self.sort = sort;
                }
            }
        });
        egui::ScrollArea::vertical().show_rows(ui, 26.0, rows.len(), |ui, range| {
            for (index, state, age, egress, active, name) in &rows[range] {
                ui.horizontal(|ui| {
                    if ui
                        .add_sized([width, 26.0], egui::Button::new(name.as_ref()).frame(false))
                        .clicked()
                    {
                        self.selected_account = Some(*index);
                    }
                    for label in [
                        age.map_or_else(|| "–".into(), |value| format!("{value} d")),
                        active.map_or_else(|| "–".into(), |value| format!("{value:.1} h")),
                        state.storage.map_or_else(|| "–".into(), format_bytes),
                        egress.map_or_else(|| "–".into(), format_bytes),
                    ] {
                        ui.add_sized([width, 26.0], egui::Label::new(label));
                    }
                });
            }
        });
    }
}

impl eframe::App for Metrics {
    fn ui(&mut self, ui: &mut Ui, _: &mut eframe::Frame) {
        workspace_rs::font::poll(ui.ctx());
        self.updates(ui.ctx());
        let Some(history) = self.history.clone() else {
            self.onboarding(ui);
            return;
        };
        let tokens = re_ui::design_tokens_of_visuals(ui.visuals());
        let background = if ui.visuals().dark_mode {
            tokens.viewport_background
        } else {
            tokens
                .viewport_background
                .lerp_to_gamma(tokens.strong_fg_color, 0.055)
        };
        ui.painter().rect_filled(ui.max_rect(), 0.0, background);
        let available_width = ui.available_width();
        let toolbar = egui::Area::new(egui::Id::unique("metrics toolbar"))
            .anchor(Align2::CENTER_TOP, [0.0, 16.0])
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                ui.set_max_width((available_width - 36.0).min(560.0));
                self.controls(ui);
            });
        let source_height = self.source_button(ui.ctx());
        Frame::new()
            .inner_margin(egui::Margin {
                left: 20,
                right: 20,
                top: 20,
                bottom: (source_height.ceil() as i8).saturating_add(24),
            })
            .show(ui, |ui| {
                ui.add_space(toolbar.response.rect.height() + 12.0);
                let title = match self.tab {
                    Tab::Overview => "Overview",
                    Tab::Retention => "Retention",
                    Tab::Accounts => "Accounts",
                };
                ui.label(bold_text(title, 30.0));
                self.state_control(ui, &history);
                ui.add_space(12.0);
                if self.query.is_none() && self.range.is_some() {
                    ui.spinner();
                }
                if self.tab == Tab::Accounts && self.selected_account.is_none() {
                    surface(ui).show(ui, |ui| self.accounts(ui, &history));
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt(self.tab as u8)
                    .show(ui, |ui| match self.tab {
                        Tab::Overview => self.overview(ui, &history),
                        Tab::Retention => self.retention(ui),
                        Tab::Accounts => self.accounts(ui, &history),
                    });
            });
    }
}

fn bold_text(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).font(FontId::new(size, egui::FontFamily::Name(Arc::from("Bold"))))
}

fn surface(ui: &Ui) -> Frame {
    let tokens = re_ui::design_tokens_of_visuals(ui.visuals());
    Frame::new()
        .fill(tokens.notification_panel_background_color)
        .stroke(Stroke::new(
            tokens.divider_stroke.width,
            tokens.divider_stroke.color.gamma_multiply(0.65),
        ))
        .corner_radius(ui.visuals().window_corner_radius)
        .inner_margin(16.0)
}

fn floating_frame(ui: &Ui) -> Frame {
    re_ui::design_tokens_of_visuals(ui.visuals()).popup_frame(ui.style())
}

type Line = (&'static str, fn(&SeriesPoint) -> Option<f64>);
type PlotSeries = (&'static str, Vec<(i64, Option<f64>)>);

struct Chart {
    title: &'static str,
    unit: Unit,
    series: Vec<PlotSeries>,
}

fn chart_columns(width: f32, chart_count: usize) -> usize {
    let fit = ((width + CHART_GAP) / (CHART_MIN_WIDTH + CHART_GAP)) as usize;
    fit.max(1).min(chart_count)
}

fn chart_grid(
    ui: &mut Ui, range: (i64, i64), selected: Option<i64>, charts: &[Chart],
) -> Option<i64> {
    let columns = chart_columns(ui.available_width(), charts.len());
    if columns == 0 {
        return None;
    }
    let cursor_group = ui.scope_id().with("metrics time cursor");
    let mut clicked = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = CHART_GAP;
        for row in charts.chunks(columns) {
            let align_headers = row.len() > 1 && row.iter().any(|chart| chart.series.len() > 1);
            ui.columns(columns, |cells| {
                for (cell, chart) in cells.iter_mut().zip(row) {
                    let selection = surface(cell)
                        .show(cell, |ui| {
                            chart_content(ui, range, selected, chart, cursor_group, align_headers)
                        })
                        .inner;
                    clicked = selection.or(clicked);
                }
            });
            ui.add_space(CHART_GAP);
        }
    });
    clicked
}

fn chart_content(
    ui: &mut Ui, (start, end): (i64, i64), selected: Option<i64>, chart: &Chart,
    cursor_group: egui::Id, align_header: bool,
) -> Option<i64> {
    let Chart { title, unit, series } = chart;
    let compact = ui.available_width() < 260.0;
    let visuals = ui.visuals();
    let tokens = re_ui::design_tokens_of_visuals(visuals);
    let colors = if visuals.dark_mode {
        [
            Color32::from_rgb(128, 163, 253),
            Color32::from_rgb(83, 203, 175),
            Color32::from_rgb(247, 173, 113),
            Color32::from_rgb(196, 149, 245),
        ]
    } else {
        [
            Color32::from_rgb(52, 91, 193),
            Color32::from_rgb(15, 123, 99),
            Color32::from_rgb(164, 85, 33),
            Color32::from_rgb(127, 73, 172),
        ]
    };
    let tick_color = visuals.weak_text_color();
    let cursor_color = visuals.selection.stroke.color;

    ui.vertical(|ui| {
        ui.label(bold_text(*title, 17.0));
        if series.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                for ((label, _), color) in series.iter().zip(colors) {
                    ui.colored_label(color, *label);
                }
            });
        }
        if align_header {
            ui.set_min_height(42.0);
        }
    });

    let axis = YAxis::new(
        *unit,
        series
            .iter()
            .flat_map(|(_, values)| values.iter().filter_map(|(_, value)| *value)),
    );
    let [bottom, top] = axis.bounds();
    let span = end - start;
    let (left, right) = if span == 0 { (start - 60_000, end + 60_000) } else { (start, end) };
    let x_axis = AxisHints::new_x()
        .tick_label_font(FontId::proportional(11.0))
        .tick_label_color(tick_color)
        .formatter(move |tick, _| {
            let format = match span {
                0..240_000 => format_description!("[hour]:[minute]:[second]"),
                240_000..DAY => format_description!("[hour]:[minute]"),
                _ if compact => format_description!("[month]-[day]"),
                _ => DATE,
            };
            format_date(tick.value.round() as i64, format)
        });
    let y_axis = AxisHints::new_y()
        .min_thickness(if compact { 52.0 } else { 72.0 })
        .tick_label_font(FontId::proportional(11.0))
        .tick_label_color(tick_color)
        .formatter(|tick, _| axis.label(tick.value));

    let plot = Plot::new(*title)
        .id(cursor_group.with(title))
        .height(154.0)
        .custom_x_axes(vec![x_axis])
        .custom_y_axes(vec![y_axis])
        .show_background(false)
        .show_grid([false, true])
        .grid_color(tokens.divider_stroke.color)
        .x_grid_spacer(move |_| time_grid(start, end))
        .y_grid_spacer(|_| {
            axis.values()
                .map(|value| GridMark { value, step_size: axis.step_size() })
                .collect()
        })
        .show_y(false)
        .cursor_color(tick_color)
        .link_cursor(cursor_group, [true, false])
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .allow_axis_zoom_drag(false)
        .sense(egui::Sense::click())
        .label_formatter(|hover| {
            let HoverPosition::Elsewhere { position } = hover else { return None };
            let time = position.x.round() as i64;
            let mut label = String::new();
            let mut has_value = false;
            for (name, points) in series {
                let Some((stamp, value)) = nearest_sample(points, time) else { continue };
                if label.is_empty() {
                    label = timestamp(stamp);
                }
                if let Some(value) = value.filter(|value| value.is_finite()) {
                    has_value = true;
                    let _ = write!(label, "\n{name} · {}", unit.label(value));
                }
            }
            if !has_value {
                if !label.is_empty() {
                    label.push('\n');
                }
                label.push_str("Missing coverage");
            }
            Some(label)
        })
        .show(ui, |plot| {
            plot.set_plot_bounds(PlotBounds::from_min_max(
                [left as f64, bottom],
                [right as f64, top],
            ));
            for ((name, values), color) in series.iter().zip(colors) {
                for segment in values.split(|(_, value)| value.is_none_or(|v| !v.is_finite())) {
                    match segment {
                        [] => {}
                        [(stamp, Some(value))] => plot.points(
                            Points::new(*name, [*stamp as f64, *value])
                                .color(color)
                                .radius(2.5)
                                .allow_hover(false),
                        ),
                        _ => {
                            let points: Vec<_> = segment
                                .iter()
                                .filter_map(|(stamp, value)| {
                                    value.map(|value| [*stamp as f64, value])
                                })
                                .collect();
                            plot.line(
                                PlotLine::new(*name, points)
                                    .color(color)
                                    .width(1.8)
                                    .allow_hover(false),
                            );
                        }
                    }
                }
            }
            if let Some(time) = selected.filter(|time| (start..=end).contains(time)) {
                plot.vline(
                    VLine::new("Selected checkpoint", time as f64)
                        .color(cursor_color)
                        .allow_hover(false),
                );
            }
        });

    if !plot.response.clicked() {
        return None;
    }
    let pointer = plot.response.interact_pointer_pos()?;
    let time = plot.transform.value_from_position(pointer).x.round() as i64;
    series
        .iter()
        .find_map(|(_, points)| nearest_sample(points, time).map(|(stamp, _)| stamp))
}

fn time_grid(start: i64, end: i64) -> Vec<GridMark> {
    let span = end - start;
    let minimum_step = if span >= DAY { DAY } else { 1_000 };
    let target_step = (span / 4).max(minimum_step);
    let step = [
        1_000,
        5_000,
        15_000,
        60_000,
        5 * 60_000,
        15 * 60_000,
        DAY / 24,
        DAY / 8,
        DAY / 4,
        DAY / 2,
        DAY,
    ]
    .into_iter()
    .find(|&step| step >= target_step)
    .unwrap_or_else(|| ((target_step + DAY - 1) / DAY) * DAY);
    let first = start + (step - start.rem_euclid(step)) % step;
    if first > end {
        return vec![GridMark { value: start as f64, step_size: step as f64 }];
    }
    (0..=(end - first) / step)
        .map(|index| GridMark { value: (first + index * step) as f64, step_size: step as f64 })
        .collect()
}

fn nearest_sample(points: &[(i64, Option<f64>)], time: i64) -> Option<(i64, Option<f64>)> {
    points
        .iter()
        .min_by_key(|(stamp, _)| stamp.abs_diff(time))
        .copied()
}

fn range_label((start, end): (i64, i64), now: i64) -> String {
    let label = |time| {
        if queries::day(time) == queries::day(now) { "Present".into() } else { date(time) }
    };
    if queries::day(start) == queries::day(end) {
        label(end)
    } else {
        format!("{} – {}", label(start), label(end))
    }
}

fn parse_date(value: &str) -> Option<i64> {
    let day = time::Date::parse(value.trim(), DATE).ok()?;
    let timestamp = day.midnight().assume_utc().unix_timestamp();
    (timestamp >= 0).then_some(timestamp * 1000)
}

fn current_time() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp() * 1000
}

fn logo_angle(turn: f32) -> f32 {
    std::f32::consts::TAU * turn * turn * (3.0 - 2.0 * turn)
}

fn preset_start(days: i64, end: i64) -> i64 {
    if days == 0 { queries::day(end) } else { end - days * DAY }
}

fn date(value: i64) -> String {
    format_date(value, DATE)
}
fn timestamp(value: i64) -> String {
    format_date(value, TIME)
}

fn format_date(value: i64, format: &[time::format_description::BorrowedFormatItem<'_>]) -> String {
    OffsetDateTime::from_unix_timestamp(value.div_euclid(1000))
        .ok()
        .and_then(|date| date.format(format).ok())
        .unwrap_or_else(|| "Unknown date".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_chart_columns_with_room_for_gaps() {
        assert_eq!(chart_columns(200.0, 9), 1);
        assert_eq!(chart_columns(971.0, 9), 1);
        assert_eq!(chart_columns(972.0, 9), 2);
        assert_eq!(chart_columns(1464.0, 9), 3);
        assert_eq!(chart_columns(1956.0, 9), 4);
        assert_eq!(chart_columns(2200.0, 3), 3);
        assert_eq!(chart_columns(1800.0, 0), 0);
    }

    #[test]
    fn chart_cards_reflow_without_changing_plot_ids() {
        let ctx = Context::default();
        re_ui::apply_style_and_install_loaders(&ctx);
        workspace_rs::install_egui_fonts(&ctx);
        let charts = ["Users", "Sharing", "Premium"].map(|title| Chart {
            title,
            unit: Unit::Count,
            series: vec![(title, vec![(0, Some(1.0)), (DAY, Some(2.0))])],
        });
        let draw = |width| {
            let mut ids = None;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width, 760.0))),
                ..Default::default()
            };
            ctx.run_ui(input, |ui| {
                Frame::new()
                    .inner_margin(egui::Margin::symmetric(20, 0))
                    .show(ui, |ui| {
                        let cursor_group = ui.scope_id().with("metrics time cursor");
                        ids = Some(
                            charts
                                .each_ref()
                                .map(|chart| cursor_group.with(chart.title)),
                        );
                        chart_grid(ui, (0, DAY), None, &charts);
                    });
            })
            .drop_without_applying_deltas();
            let ids = ids.unwrap();
            let frames = ids.map(|id| {
                *egui_plot::PlotMemory::load(&ctx, id)
                    .unwrap()
                    .transform()
                    .frame()
            });
            (ids, frames)
        };

        let (wide_ids, [first, second, third]) = draw(1100.0);
        assert_eq!(first.top(), second.top());
        assert!(first.right() < second.left());
        assert!(first.bottom() < third.top());
        let (narrow_ids, [first, second, third]) = draw(560.0);
        assert_eq!(wide_ids, narrow_ids);
        assert_eq!(first.left(), second.left());
        assert!(first.bottom() < second.top() && second.bottom() < third.top());
    }

    #[test]
    fn labels_only_the_current_utc_day_as_present() {
        let today = parse_date("2026-11-21").unwrap();
        let yesterday = today - DAY;
        let now = today + DAY / 2;
        assert_eq!(range_label((yesterday, now), now), "2026-11-20 – Present");
        assert_eq!(range_label((today, now), now), "Present");
        assert_eq!(range_label((yesterday, today - 1), now), "2026-11-20");
    }

    #[test]
    fn clicking_plot_picks_the_nearest_checkpoint_even_in_a_coverage_gap() {
        let ctx = Context::default();
        re_ui::apply_style_and_install_loaders(&ctx);
        workspace_rs::install_egui_fonts(&ctx);
        let start = 1_700_000_000_000;
        let middle = start + DAY;
        let end = middle + DAY;
        let charts = [Chart {
            title: "Users",
            unit: Unit::Count,
            series: vec![("Users", vec![(start, Some(42.0)), (middle, None), (end, Some(45.0))])],
        }];
        let pointer = egui::pos2(400.0, 75.0);
        let mut selection = None;

        let mut draw = |events| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(720.0, 240.0))),
                events,
                ..Default::default()
            };
            ctx.run_ui(input, |ui| {
                selection = chart_grid(ui, (start, end), None, &charts);
            })
            .drop_without_applying_deltas();
        };
        draw(vec![egui::Event::PointerMoved(pointer)]);
        for pressed in [true, false] {
            draw(vec![egui::Event::PointerButton {
                pos: pointer,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }]);
        }
        assert_eq!(selection, Some(middle));
    }
}
