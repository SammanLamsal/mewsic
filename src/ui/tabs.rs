use eframe::egui;

use crate::app::{Action, View};
use crate::queues::{OpenQueues, QueueStore};

pub fn show(
    ui: &mut egui::Ui,
    queues: &OpenQueues,
    store: &QueueStore,
    view: View,
    actions: &mut Vec<Action>,
) {
    ui.horizontal_wrapped(|ui| {
        if ui
            .selectable_label(view == View::Library, "Library")
            .clicked()
        {
            actions.push(Action::ShowView(View::Library));
        }
        ui.separator();

        for (index, tab) in queues.tabs().iter().enumerate() {
            let is_active = queues.active() == Some(index);
            show_queue_tab(ui, index, &tab.queue.name, is_active, view, actions);
        }

        if ui.button("+").on_hover_text("New queue").clicked() {
            actions.push(Action::AskNewQueue {
                name_hint: "Queue".to_string(),
                tracks: Vec::new(),
            });
        }

        ui.menu_button("Saved queues", |ui| {
            show_saved_queues_menu(ui, queues, store, actions);
        });

        if ui
            .selectable_label(view == View::Settings, "⚙")
            .on_hover_text("Settings")
            .clicked()
        {
            actions.push(Action::ShowView(View::Settings));
        }
    });
}

fn show_queue_tab(
    ui: &mut egui::Ui,
    index: usize,
    name: &str,
    is_active: bool,
    view: View,
    actions: &mut Vec<Action>,
) {
    let label = if is_active {
        format!("▶ {name}")
    } else {
        name.to_string()
    };

    let response = ui
        .selectable_label(view == View::Queue(index), label)
        .on_hover_text(if is_active {
            "Playing in MPD"
        } else {
            "Right-click for options"
        });

    if response.clicked() {
        actions.push(Action::ShowView(View::Queue(index)));
    }

    response.context_menu(|ui| {
        if ui.button("Play").clicked() {
            actions.push(Action::ResumeTab(index));
        }
        if ui.button("Rename…").clicked() {
            actions.push(Action::AskRenameTab(index));
        }
        if ui.button("Close").clicked() {
            actions.push(Action::CloseTab(index));
        }
    });

    if ui
        .small_button("×")
        .on_hover_text("Close tab (the queue stays saved)")
        .clicked()
    {
        actions.push(Action::CloseTab(index));
    }
    ui.add_space(6.0);
}

fn show_saved_queues_menu(
    ui: &mut egui::Ui,
    queues: &OpenQueues,
    store: &QueueStore,
    actions: &mut Vec<Action>,
) {
    let names = store.saved_names();
    if names.is_empty() {
        ui.weak("No saved queues");
        return;
    }

    for name in names {
        ui.horizontal(|ui| {
            let is_open = queues.find(&name).is_some();
            let open_button = ui.add_enabled(!is_open, egui::Button::new(&name));
            if open_button.clicked() {
                actions.push(Action::OpenSavedQueue(name.clone()));
            }
            if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                actions.push(Action::AskDeleteSavedQueue(name.clone()));
            }
        });
    }
}
