// SPDX-License-Identifier: MPL-2.0

use crate::cast::{self, Backend, BackendActionResult, CastDisplay, RuntimeStatus};
use crate::config::Config;
use crate::fl;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::platform_specific::shell::wayland::commands::popup::{destroy_popup, get_popup};
use cosmic::iced::{Alignment, Length, Limits, Subscription, window::Id};
use cosmic::prelude::*;
use cosmic::widget;

/// The application model stores app-specific state used to describe its interface and
/// drive its logic.
#[derive(Default)]
pub struct AppModel {
    /// Application state which is managed by the COSMIC runtime.
    core: cosmic::Core,
    /// The popup id.
    popup: Option<Id>,
    /// Configuration data that persists between application runs.
    config: Config,
    /// The latest detected local casting runtime state.
    runtime: Option<RuntimeStatus>,
    /// Whether the applet is waiting for a backend command to finish.
    busy: bool,
    /// Last user-visible result from a backend action.
    last_message: Option<String>,
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    UpdateConfig(Config),
    Refresh,
    RuntimeChecked(RuntimeStatus),
    OpenPicker(Backend),
    StartStream(Backend, CastDisplay),
    StopStream(Backend, CastDisplay),
    StopCasting(Backend),
    BackendActionFinished(BackendActionResult),
}

/// Create a COSMIC application from the app model
impl cosmic::Application for AppModel {
    /// The async executor that will be used to run your application's commands.
    type Executor = cosmic::executor::Default;

    /// Data that your application receives to its init method.
    type Flags = ();

    /// Messages which the application and its widgets will emit.
    type Message = Message;

    /// Unique identifier in RDNN (reverse domain name notation) format.
    const APP_ID: &'static str = "io.github.bigolpants.CosmicCastExt";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    /// Initializes the application with any given flags and startup commands.
    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        // Construct the app model with the runtime's core.
        let app = AppModel {
            core,
            config: cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                .map(|context| match Config::get_entry(&context) {
                    Ok(config) => config,
                    Err((_errors, config)) => {
                        // for why in errors {
                        //     tracing::error!(%why, "error loading app config");
                        // }

                        config
                    }
                })
                .unwrap_or_default(),
            ..Default::default()
        };

        (app, runtime_check_task())
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    /// Describes the interface based on the current state of the application model.
    ///
    /// The applet's button in the panel will be drawn using the main view method.
    /// This view should emit messages to toggle the applet's popup window, which will
    /// be drawn using the `view_window` method.
    fn view(&self) -> Element<'_, Self::Message> {
        self.core
            .applet
            .icon_button("display-symbolic")
            .on_press(Message::TogglePopup)
            .into()
    }

    /// The applet's popup window will be drawn using this view method. If there are
    /// multiple poups, you may match the id parameter to determine which popup to
    /// create a view for.
    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let content_list = self.cast_controls();

        self.core.applet.popup_container(content_list).into()
    }

    /// Register subscriptions for this application.
    ///
    /// Subscriptions are long-lived async tasks running in the background which
    /// emit messages to the application through a channel. They may be conditionally
    /// activated by selectively appending to the subscription batch, and will
    /// continue to execute for the duration that they remain in the batch.
    fn subscription(&self) -> Subscription<Self::Message> {
        // Watch for application configuration changes.
        self.core()
            .watch_config::<Config>(Self::APP_ID)
            .map(|update| {
                // for why in update.errors {
                //     tracing::error!(?why, "app config error");
                // }

                Message::UpdateConfig(update.config)
            })
    }

    /// Handles messages emitted by the application and its widgets.
    ///
    /// Tasks may be returned for asynchronous execution of code in the background
    /// on the application's async runtime. The application will not exit until all
    /// tasks are finished.
    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::UpdateConfig(config) => {
                self.config = config;
            }
            Message::Refresh => {
                self.busy = true;
                self.last_message = None;
                return runtime_check_task();
            }
            Message::RuntimeChecked(status) => {
                self.runtime = Some(status);
                self.busy = false;
            }
            Message::OpenPicker(backend) => {
                self.busy = true;
                self.last_message = None;
                return backend_action_task(cast::open_picker(backend));
            }
            Message::StartStream(backend, display) => {
                self.busy = true;
                self.last_message = None;
                return backend_action_task(cast::start_stream(backend, display));
            }
            Message::StopStream(backend, display) => {
                self.busy = true;
                self.last_message = None;
                return backend_action_task(cast::stop_stream(backend, display));
            }
            Message::StopCasting(backend) => {
                self.busy = true;
                self.last_message = None;
                return backend_action_task(cast::stop_casting(backend));
            }
            Message::BackendActionFinished(result) => {
                self.runtime = Some(result.status);
                self.last_message = Some(result.message);
                self.busy = false;
            }
            Message::TogglePopup => {
                return if let Some(p) = self.popup.take() {
                    destroy_popup(p)
                } else {
                    let new_id = Id::unique();
                    self.popup.replace(new_id);
                    let mut popup_settings = self.core.applet.get_popup_settings(
                        self.core.main_window_id().unwrap(),
                        new_id,
                        None,
                        None,
                        None,
                    );
                    popup_settings.positioner.size_limits = Limits::NONE
                        .max_width(372.0)
                        .min_width(300.0)
                        .min_height(200.0)
                        .max_height(1080.0);
                    Task::batch([get_popup(popup_settings), runtime_check_task()])
                };
            }
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                }
            }
        }
        Task::none()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

impl AppModel {
    fn cast_controls(&self) -> widget::ListColumn<'_, Message> {
        let status = self.runtime.as_ref();
        let summary = status.map_or_else(|| fl!("checking-support"), RuntimeStatus::summary);

        let mut list = widget::list_column()
            .add(
                widget::settings::item::builder(fl!("wireless-display"))
                    .description(summary)
                    .control(self.primary_action(status)),
            )
            .add(
                widget::settings::item::builder(fl!("network-manager"))
                    .description(
                        status.map_or_else(
                            || fl!("checking"),
                            |status| status.network_manager.label(),
                        ),
                    )
                    .control(widget::text::caption(fl!("required"))),
            )
            .add(
                widget::settings::item::builder(fl!("wi-fi-direct"))
                    .description(
                        status.map_or_else(|| fl!("checking"), |status| status.wifi_direct.label()),
                    )
                    .control(widget::text::caption(fl!("miracast"))),
            )
            .add(
                widget::settings::item::builder(fl!("miracast-displays"))
                    .description(Self::miracast_display_summary(status))
                    .control(widget::text::caption(fl!("filtered"))),
            );

        if let Some(status) = status {
            if status.chromecast_count > 0 {
                list = list.add(
                    widget::settings::item::builder(fl!("chromecast-devices"))
                        .description(format!(
                            "{} {}",
                            status.chromecast_count,
                            fl!("chromecast-ignored")
                        ))
                        .control(widget::text::caption(fl!("ignored"))),
                );
            }

            for display in &status.miracast_displays {
                list = list.add(self.miracast_display_item(status, display));
            }

            let backend_text = status.backend.as_ref().map_or_else(
                || fl!("not-installed"),
                |backend| backend.detail().to_owned(),
            );

            list = list.add(
                widget::settings::item::builder(fl!("cast-backend"))
                    .description(backend_text)
                    .control(widget::text::caption(
                        status
                            .backend
                            .as_ref()
                            .map_or_else(|| fl!("missing"), ToString::to_string),
                    )),
            );

            for note in &status.notes {
                list = list.add(
                    widget::settings::item::builder(fl!("note"))
                        .description(note)
                        .control(widget::button::text(fl!("refresh")).on_press(Message::Refresh)),
                );
            }
        }

        if let Some(message) = &self.last_message {
            list = list.add(
                widget::settings::item::builder(fl!("last-result"))
                    .description(message)
                    .control(widget::text::caption("")),
            );
        }

        list
    }

    fn miracast_display_item(
        &self,
        status: &RuntimeStatus,
        display: &CastDisplay,
    ) -> Element<'_, Message> {
        let backend = status.backend.clone();
        let title = display.name.clone();
        let description = display.state.label();
        let action_display = display.clone();
        let action = if display.state.is_active() {
            widget::button::destructive(fl!("stop")).on_press_maybe(
                (backend.is_some() && !self.busy).then(|| {
                    Message::StopStream(backend.expect("backend checked above"), action_display)
                }),
            )
        } else {
            widget::button::suggested(fl!("cast")).on_press_maybe(
                (backend.is_some() && !self.busy).then(|| {
                    Message::StartStream(backend.expect("backend checked above"), action_display)
                }),
            )
        };

        widget::settings::item::builder(title)
            .description(description)
            .control(action)
            .into()
    }

    fn miracast_display_summary(status: Option<&RuntimeStatus>) -> String {
        let Some(status) = status else {
            return fl!("checking");
        };

        if !status.display_scan.is_available() {
            return status.display_scan.label();
        }

        if status.miracast_displays.is_empty() {
            return fl!("no-miracast-displays");
        }

        status
            .miracast_displays
            .iter()
            .map(|display| display.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn primary_action(&self, status: Option<&RuntimeStatus>) -> Element<'_, Message> {
        let backend = status.and_then(|status| status.backend.clone());
        let running = status.is_some_and(|status| status.running);

        let open = widget::button::suggested(fl!("open-picker")).on_press_maybe(
            (backend.clone().is_some() && !self.busy)
                .then(|| Message::OpenPicker(backend.clone().expect("backend checked above"))),
        );

        let stop = widget::button::destructive(fl!("stop")).on_press_maybe(
            (backend.is_some() && running && !self.busy)
                .then(|| Message::StopCasting(backend.expect("backend checked above"))),
        );

        widget::row::with_capacity(3)
            .spacing(8)
            .align_y(Alignment::Center)
            .push(open)
            .push(stop)
            .push(
                widget::button::standard(if self.busy {
                    fl!("checking")
                } else {
                    fl!("refresh")
                })
                .on_press_maybe((!self.busy).then_some(Message::Refresh)),
            )
            .width(Length::Shrink)
            .into()
    }
}

fn runtime_check_task() -> Task<cosmic::Action<Message>> {
    Task::perform(cast::inspect(), |status| {
        cosmic::Action::App(Message::RuntimeChecked(status))
    })
}

fn backend_action_task(
    future: impl std::future::Future<Output = BackendActionResult> + Send + 'static,
) -> Task<cosmic::Action<Message>> {
    Task::perform(future, |result| {
        cosmic::Action::App(Message::BackendActionFinished(result))
    })
}
