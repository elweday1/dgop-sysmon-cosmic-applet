// SPDX-License-Identifier: GPL-3.0-only

use cosmic::{
    Element, Task, app,
    applet::cosmic_panel_config::PanelAnchor,
    iced::{
        Alignment, Length, Rectangle, Subscription,
        alignment,
        futures::{SinkExt, Stream, channel::mpsc},
        platform_specific::shell::wayland::commands::popup::destroy_popup,
        widget::{column, row},
        window,
    },
    theme,
    widget::{
        Id, autosize, button, container, context_menu, icon, menu, tooltip,
        rectangle_tracker::*, scrollable, text,
    },
};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

use crate::{
    dgop::{self, Cursors, Meta, ProcSignal, Process},
    fl, format,
};

static AUTOSIZE_MAIN_ID: LazyLock<Id> = LazyLock::new(|| Id::new("autosize-main"));
static POPUP_AUTOSIZE_ID: LazyLock<Id> = LazyLock::new(|| Id::new("autosize-popup"));

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_PROCESS_ROWS: usize = 300;
const POPUP_WIDTH: f32 = 700.0;
const POPUP_HEIGHT: f32 = 621.0;
const ROW_PAD_X: u16 = 12;
const NAME_MAX_CHARS: usize = 60;
const CARD_HEIGHT: f32 = 60.0;
const COL_USER: f32 = 112.0;
const COL_PID: f32 = 64.0;
const COL_CPU: f32 = 72.0;
const COL_MEM: f32 = 96.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    #[default]
    Cpu,
    Memory,
    Name,
    User,
    Pid,
}

impl SortBy {
    fn default_descending(self) -> bool {
        matches!(self, SortBy::Cpu | SortBy::Memory)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessAction {
    Signal(i32, ProcSignal),
    CopyPid(i32),
    CopyCommand(i32),
}

impl menu::Action for ProcessAction {
    type Message = Message;

    fn message(&self) -> Message {
        match *self {
            ProcessAction::Signal(pid, signal) => Message::Signal { pid, signal },
            ProcessAction::CopyPid(pid) => Message::CopyText(pid.to_string()),
            ProcessAction::CopyCommand(pid) => Message::CopyCommand(pid),
        }
    }
}

#[derive(Hash)]
struct SysmonSub {
    full: bool,
}

pub struct Window {
    core: app::Core,
    popup: Option<window::Id>,
    rectangle_tracker: Option<RectangleTracker<u32>>,
    rectangle: Rectangle,
    search: String,
    sort: SortBy,
    descending: bool,
    meta: Meta,
    error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    CloseRequested(window::Id),
    Rectangle(RectangleUpdate<u32>),
    Stats(Result<Meta, String>),
    Search(String),
    Sort(SortBy),
    Signal { pid: i32, signal: ProcSignal },
    CopyText(String),
    CopyCommand(i32),
    Noop,
}

impl Window {
    fn processes(&self) -> &[Process] {
        self.meta.processes.as_deref().unwrap_or(&[])
    }

    fn visible_processes(&self) -> Vec<&Process> {
        let needle = self.search.trim().to_lowercase();
        let mut procs: Vec<&Process> = self
            .processes()
            .iter()
            .filter(|p| {
                needle.is_empty()
                    || p.command.to_lowercase().contains(&needle)
                    || p.full_command.to_lowercase().contains(&needle)
                    || p.pid.to_string().contains(&needle)
                    || p.username.to_lowercase().contains(&needle)
            })
            .collect();

        procs.sort_by(|a, b| {
            let ordering = match self.sort {
                SortBy::Cpu => a.cpu.total_cmp(&b.cpu),
                SortBy::Memory => a.memory_kb.cmp(&b.memory_kb),
                SortBy::Name => a.command.to_lowercase().cmp(&b.command.to_lowercase()),
                SortBy::User => a.username.cmp(&b.username),
                SortBy::Pid => a.pid.cmp(&b.pid),
            };
            let ordering = if self.descending {
                ordering.reverse()
            } else {
                ordering
            };
            ordering.then(a.pid.cmp(&b.pid))
        });
        procs
    }

    fn panel_memory(&self) -> String {
        self.meta
            .memory
            .as_ref()
            .map_or("--".to_owned(), |mem| format::memory_short(mem.used))
    }

    fn unavailable_card(&self) -> Element<'_, Message> {
        let mut content = column![
            text::heading(fl!("unavailable-title")),
            text::body(fl!("unavailable-body")),
        ]
        .spacing(8);
        if let Some(error) = &self.error {
            content = content.push(text::caption(error.clone()));
        }
        container(content)
            .padding(16)
            .class(theme::Container::Card)
            .width(Length::Fill)
            .into()
    }

    fn stat<'a>(
        icon_name: &'static str,
        label: String,
        value: String,
        value_size: Option<f32>,
    ) -> Element<'a, Message> {
        let value: Element<'a, Message> = match value_size {
            Some(size) => text::body(value).size(size).into(),
            None => text::body(value).into(),
        };
        let content = row![
            icon::from_name(icon_name).size(20).symbolic(true),
            column![text::caption(label), value].spacing(2),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        container(content)
            .padding([8, 12])
            .height(Length::Fixed(CARD_HEIGHT))
            .width(Length::Fill)
            .align_y(alignment::Vertical::Center)
            .class(theme::Container::Card)
            .into()
    }

    fn summary(&self) -> Element<'_, Message> {
        let mut stats: Vec<Element<'_, Message>> = Vec::new();

        if let Some(cpu) = &self.meta.cpu {
            stats.push(Self::stat(
                "xsi-cpu-symbolic",
                fl!("cpu"),
                format!("{:.1}%", cpu.usage),
                None,
            ));
        }
        if let Some(mem) = &self.meta.memory {
            stats.push(Self::stat(
                "xsi-ram-symbolic",
                fl!("memory"),
                format!(
                    "{} / {}",
                    format::memory_kb(mem.used),
                    format::memory_kb(mem.total)
                ),
                Some(12.0),
            ));
        }
        if let Some(net) = &self.meta.net_rate {
            let rx: f64 = net.interfaces.iter().map(|i| i.rx_rate).sum();
            let tx: f64 = net.interfaces.iter().map(|i| i.tx_rate).sum();
            stats.push(Self::stat(
                "network-wired-symbolic",
                fl!("network"),
                format!("↓ {}  ↑ {}", format::rate(rx), format::rate(tx)),
                Some(11.0),
            ));
        }
        if let Some(system) = &self.meta.system {
            stats.push(Self::stat(
                "system-run-symbolic",
                fl!("processes"),
                format!("{} · {}", system.processes, system.threads),
                None,
            ));
        }

        row(stats).spacing(8).width(Length::Fill).into()
    }

    fn cell<'a>(
        content: impl Into<Element<'a, Message>>,
        width: Length,
        align: alignment::Horizontal,
    ) -> Element<'a, Message> {
        container(content).width(width).align_x(align).into()
    }

    fn header_cell<'a>(
        &self,
        label: String,
        sort: Option<SortBy>,
        width: Length,
        align: alignment::Horizontal,
    ) -> Element<'a, Message> {
        let content: Element<'a, Message> = match sort {
            Some(sort) => {
                let arrow = match (self.sort == sort, self.descending) {
                    (true, true) => " ▼",
                    (true, false) => " ▲",
                    _ => "",
                };
                button::text(format!("{label}{arrow}"))
                    .on_press(Message::Sort(sort))
                    .padding([4, 0])
                    .into()
            }
            None => text::caption(label).into(),
        };
        Self::cell(content, width, align)
    }

    fn process_menu(&self, process: &Process) -> Vec<menu::Tree<Message>> {
        let pid = process.pid;
        menu::items(
            &HashMap::new(),
            vec![
                menu::Item::Button(
                    fl!("process-terminate"),
                    None,
                    ProcessAction::Signal(pid, ProcSignal::Terminate),
                ),
                menu::Item::Button(
                    fl!("process-force-kill"),
                    None,
                    ProcessAction::Signal(pid, ProcSignal::Kill),
                ),
                menu::Item::Divider,
                menu::Item::Button(
                    fl!("process-stop"),
                    None,
                    ProcessAction::Signal(pid, ProcSignal::Stop),
                ),
                menu::Item::Button(
                    fl!("process-continue"),
                    None,
                    ProcessAction::Signal(pid, ProcSignal::Continue),
                ),
                menu::Item::Divider,
                menu::Item::Button(fl!("process-copy-pid"), None, ProcessAction::CopyPid(pid)),
                menu::Item::Button(
                    fl!("process-copy-command"),
                    None,
                    ProcessAction::CopyCommand(pid),
                ),
            ],
        )
    }

    fn processes_view(&self) -> Element<'_, Message> {
        let procs = self.visible_processes();
        let count = procs.len();

        let search = cosmic::widget::search_input(fl!("search-placeholder"), self.search.as_str())
            .on_input(Message::Search)
            .width(Length::Fill);

        let left = alignment::Horizontal::Left;
        let right = alignment::Horizontal::Right;
        let header = row![
            self.header_cell(fl!("sort-name"), Some(SortBy::Name), Length::Fill, left),
            self.header_cell(fl!("sort-user"), Some(SortBy::User), Length::Fixed(COL_USER), left),
            self.header_cell(fl!("sort-pid"), Some(SortBy::Pid), Length::Fixed(COL_PID), right),
            self.header_cell(fl!("sort-cpu"), Some(SortBy::Cpu), Length::Fixed(COL_CPU), right),
            self.header_cell(fl!("sort-memory"), Some(SortBy::Memory), Length::Fixed(COL_MEM), right),
        ]
        .align_y(Alignment::Center);

        let mut list = column![container(header).padding([0, ROW_PAD_X])].spacing(2);
        for process in procs.iter().take(MAX_PROCESS_ROWS) {
            let full = if process.full_command.is_empty() {
                process.command.as_str()
            } else {
                process.full_command.as_str()
            };
            let name_cell = tooltip(
                text::body(format::truncate(full, NAME_MAX_CHARS)),
                text::body(full.to_owned()),
                cosmic::iced::widget::tooltip::Position::Bottom,
            );
            let cells = row![
                Self::cell(name_cell, Length::Fill, left),
                Self::cell(text::caption(process.username.clone()), Length::Fixed(COL_USER), left),
                Self::cell(text::caption(process.pid.to_string()), Length::Fixed(COL_PID), right),
                Self::cell(text::body(format!("{:.1}%", process.cpu)), Length::Fixed(COL_CPU), right),
                Self::cell(text::body(format::memory_kb(process.memory_kb)), Length::Fixed(COL_MEM), right),
            ]
            .align_y(Alignment::Center);

            let row_button = button::custom(cells)
                .class(theme::Button::AppletMenu)
                .padding([4, ROW_PAD_X])
                .width(Length::Fill)
                .on_press(Message::Noop);
            list = list.push(context_menu(row_button, Some(self.process_menu(process))));
        }

        column![
            search,
            scrollable(list).height(Length::Fill),
            text::caption(fl!("process-count", count = count)),
        ]
        .spacing(8)
        .height(Length::Fill)
        .into()
    }
}

fn sysmon_stream(sub: &SysmonSub) -> impl Stream<Item = Message> + use<> {
    let full = sub.full;
    cosmic::iced::stream::channel(4, move |mut output: mpsc::Sender<Message>| async move {
        let mut cursors = Cursors::default();
        loop {
            let result = dgop::poll(full, &mut cursors).await;
            if output.send(Message::Stats(result)).await.is_err() {
                break;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    })
}

impl cosmic::Application for Window {
    type Message = Message;
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    const APP_ID: &str = "com.system76.CosmicAppletSysmon";

    fn init(core: app::Core, _flags: Self::Flags) -> (Self, app::Task<Self::Message>) {
        (
            Self {
                core,
                popup: None,
                rectangle_tracker: None,
                rectangle: Rectangle::default(),
                search: String::new(),
                sort: SortBy::default(),
                descending: SortBy::default().default_descending(),
                meta: Meta::default(),
                error: None,
            },
            Task::none(),
        )
    }

    fn core(&self) -> &app::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut app::Core {
        &mut self.core
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            rectangle_tracker_subscription(0).map(|e| Message::Rectangle(e.1)),
            Subscription::run_with(
                SysmonSub {
                    full: self.popup.is_some(),
                },
                sysmon_stream,
            ),
        ])
    }

    fn update(&mut self, message: Self::Message) -> app::Task<Self::Message> {
        match message {
            Message::TogglePopup => {
                if let Some(p) = self.popup.take() {
                    return destroy_popup(p);
                } else {
                    return cosmic::surface::surface_task(cosmic::surface::action::app_popup(
                        |_| Default::default(),
                        |app: &mut Self| {
                            let new_id = window::Id::unique();
                            app.popup = Some(new_id);

                            let mut popup_settings = app.core.applet.get_popup_settings(
                                app.core.main_window_id().unwrap(),
                                new_id,
                                None,
                                None,
                                None,
                            );
                            let Rectangle {
                                x,
                                y,
                                width,
                                height,
                            } = app.rectangle;
                            popup_settings.positioner.anchor_rect = Rectangle::<i32> {
                                x: x.max(1.) as i32,
                                y: y.max(1.) as i32,
                                width: width.max(1.) as i32,
                                height: height.max(1.) as i32,
                            };

                            popup_settings.positioner.size = None;
                            popup_settings
                        },
                        None,
                    ));
                }
            }
            Message::CloseRequested(id) => {
                if Some(id) == self.popup {
                    self.popup = None;
                }
            }
            Message::Rectangle(u) => match u {
                RectangleUpdate::Rectangle(r) => {
                    self.rectangle = r.1;
                }
                RectangleUpdate::Init(tracker) => {
                    self.rectangle_tracker = Some(tracker);
                }
            },
            Message::Stats(result) => match result {
                Ok(meta) => {
                    self.meta = meta;
                    self.error = None;
                }
                Err(err) => {
                    tracing::warn!("dgop poll failed: {err}");
                    self.error = Some(err);
                }
            },
            Message::Search(search) => self.search = search,
            Message::Sort(sort) => {
                if self.sort == sort {
                    self.descending = !self.descending;
                } else {
                    self.sort = sort;
                    self.descending = sort.default_descending();
                }
            }
            Message::Signal { pid, signal } => {
                if let Err(err) = dgop::signal(pid, signal) {
                    tracing::error!("failed to signal process {pid}: {err}");
                }
            }
            Message::CopyText(text) => return cosmic::iced::clipboard::write(text),
            Message::Noop => {}
            Message::CopyCommand(pid) => {
                if let Some(process) = self.processes().iter().find(|p| p.pid == pid) {
                    return cosmic::iced::clipboard::write(process.full_command.clone());
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let horizontal = matches!(
            self.core.applet.anchor,
            PanelAnchor::Top | PanelAnchor::Bottom
        );
        let mem = self.panel_memory();

        let content: Element<'_, Message> = if horizontal {
            row![
                icon::from_name("xsi-ram-symbolic")
                    .size(self.core.applet.suggested_size(true).0)
                    .symbolic(true),
                self.core.applet.text(mem),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into()
        } else {
            column![self.core.applet.text(mem)]
                .align_x(Alignment::Center)
                .into()
        };

        let button = button::custom(content)
            .padding(if horizontal {
                [0, self.core.applet.suggested_padding(true).0]
            } else {
                [self.core.applet.suggested_padding(true).0, 0]
            })
            .on_press_down(Message::TogglePopup)
            .class(cosmic::theme::Button::AppletIcon);

        autosize::autosize(
            if let Some(tracker) = self.rectangle_tracker.as_ref() {
                Element::from(tracker.container(0, button).ignore_bounds(true))
            } else {
                button.into()
            },
            AUTOSIZE_MAIN_ID.clone(),
        )
        .into()
    }

    fn view_window(&self, _id: window::Id) -> Element<'_, Message> {
        let space_s = theme::active().cosmic().spacing.space_s;
        let body: Element<'_, Message> = if self.error.is_some() && self.meta.cpu.is_none() {
            self.unavailable_card()
        } else if self.meta.cpu.is_none() {
            text::body(fl!("no-data")).into()
        } else {
            self.processes_view()
        };

        let header = row![
            text::heading(fl!("system-monitor")).width(Length::Fill),
            text::caption(format::uptime()),
        ]
        .align_y(Alignment::Center);

        let content = column![header, self.summary(), body]
            .spacing(space_s)
            .padding(space_s)
            .width(Length::Fixed(POPUP_WIDTH))
            .height(Length::Fixed(POPUP_HEIGHT));

        autosize::autosize(
            container(content).style(|theme: &cosmic::Theme| {
                let cosmic = theme.cosmic();
                let bg = cosmic.background(theme.transparent);
                cosmic::iced::widget::container::Style {
                    text_color: Some(bg.on.into()),
                    background: Some(cosmic::iced::Color::from(bg.base).into()),
                    border: cosmic::iced::Border {
                        radius: cosmic.corner_radii.radius_m.into(),
                        width: 1.0,
                        color: bg.divider.into(),
                    },
                    ..Default::default()
                }
            }),
            POPUP_AUTOSIZE_ID.clone(),
        )
        .limits(
            cosmic::iced::Limits::NONE
                .min_width(1.0)
                .max_width(POPUP_WIDTH)
                .min_height(1.0)
                .max_height(POPUP_HEIGHT),
        )
        .into()
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::CloseRequested(id))
    }
}
