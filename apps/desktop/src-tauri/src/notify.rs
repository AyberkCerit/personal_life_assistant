//! How reminders reach the user: a Windows notification with Done / Snooze buttons (FR-TSK-012)
//! and, while the window is open, an in-app banner via events (decision 3, FR-TSK-015).

use std::sync::mpsc::Sender;
use std::sync::Mutex;

use pla_core::reminders::DueReminder;
use tauri::{AppHandle, Emitter};

use crate::scheduler::{Notifier, SchedCommand};

pub struct Strings {
    pub done: &'static str,
    pub snooze: &'static str,
    pub reminder: &'static str,
    pub missed_title: &'static str,
    pub missed_body: &'static str,
    pub problem_title: &'static str,
    pub tray_title: &'static str,
    pub tray_body: &'static str,
}

const TR: Strings = Strings {
    done: "Tamamlandı",
    snooze: "10 dk ertele",
    reminder: "Hatırlatıcı",
    missed_title: "Kaçırılan hatırlatıcılar",
    missed_body: "PLA kapalıyken zamanı gelen hatırlatıcılar var. Listeyi uygulamada görebilirsin.",
    problem_title: "PLA arka plan işi başarısız oldu",
    tray_title: "PLA çalışmaya devam ediyor",
    tray_body: "Pencere kapandı ama hatırlatıcılar çalışıyor. Tamamen kapatmak için tepsi simgesinden Çık'ı seç.",
};

const EN: Strings = Strings {
    done: "Done",
    snooze: "Snooze 10 min",
    reminder: "Reminder",
    missed_title: "Missed reminders",
    missed_body: "Some reminders came due while PLA was closed. See the list in the app.",
    problem_title: "A PLA background job failed",
    tray_title: "PLA keeps running",
    tray_body: "The window is closed but reminders still work. To quit, choose Quit from the tray icon.",
};

/// Turkish when the Windows UI language is Turkish, otherwise English (as FR-SET-003).
pub fn strings() -> &'static Strings {
    #[cfg(windows)]
    {
        // SAFETY: no arguments, returns a LANGID.
        let lang = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
        if lang & 0x3ff == 0x1f {
            return &TR;
        }
    }
    &EN
}

fn app_id() -> &'static str {
    // Decision 8: an unpackaged dev build has no registered app id.
    if cfg!(debug_assertions) {
        tauri_winrt_notification::Toast::POWERSHELL_APP_ID
    } else {
        "dev.ayberkcerit.pla"
    }
}

pub fn show_info(title: &str, body: &str) {
    #[cfg(windows)]
    {
        if let Err(e) = tauri_winrt_notification::Toast::new(app_id()).title(title).text1(body).show() {
            eprintln!("PLA: notification failed: {e}");
        }
    }
}

pub struct AppNotifier {
    app: AppHandle,
    commands: Mutex<Sender<SchedCommand>>,
}

impl AppNotifier {
    pub fn new(app: AppHandle, commands: Sender<SchedCommand>) -> Self {
        Self { app, commands: Mutex::new(commands) }
    }
}

impl Notifier for AppNotifier {
    fn reminder(&self, reminder: &DueReminder) {
        let _ = self.app.emit("reminder-due", reminder);
        #[cfg(windows)]
        {
            let s = strings();
            let tx = self.commands.lock().expect("commands lock").clone();
            let time = reminder.notify_at.get(11..16).unwrap_or_default().to_owned();
            let shown = tauri_winrt_notification::Toast::new(app_id())
                .scenario(tauri_winrt_notification::Scenario::Reminder) // stays until answered
                .title(&reminder.title)
                .text1(&format!("{} · {time}", s.reminder))
                .add_button(s.done, &format!("done:{}", reminder.task_id))
                .add_button(s.snooze, &format!("snooze:{}", reminder.task_id))
                .on_activated(move |action| {
                    match action.as_deref().and_then(|a| a.split_once(':')) {
                        Some(("done", id)) => {
                            let _ = tx.send(SchedCommand::Done(id.to_owned()));
                        }
                        Some(("snooze", id)) => {
                            let _ = tx.send(SchedCommand::Snooze(id.to_owned()));
                        }
                        _ => {}
                    }
                    Ok(())
                })
                .show();
            if let Err(e) = shown {
                eprintln!("PLA: notification failed: {e}");
            }
        }
    }

    fn missed(&self, reminders: &[DueReminder]) {
        let _ = self.app.emit("missed-reminders", reminders);
        let s = strings();
        show_info(&format!("{} ({})", s.missed_title, reminders.len()), s.missed_body);
    }

    fn problem(&self, message: &str) {
        let _ = self.app.emit("background-problem", message);
        show_info(strings().problem_title, message);
    }

    fn tasks_changed(&self) {
        let _ = self.app.emit("tasks-changed", ());
    }
}
