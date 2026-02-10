use crate::monitor::event::{ConnectionStatus, MonitorEvent};
use crate::monitor::message::MonitorMessage;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration as StdDuration, Instant};
use uuid::Uuid;

const CHANNEL_BUFFER_CAPACITY: usize = 200;
const FLASH_DURATION: StdDuration = StdDuration::from_millis(600);

/// Mutable state shared between the network event pump and the UI renderer.
pub struct MonitorState {
    /// Messages grouped by channel.
    pub channels: HashMap<String, ChannelState>,
    /// Queue of blocking items that still need answers/approvals.
    pub queue: VecDeque<QueueItem>,
    /// Index of the focused queue item.
    pub queue_index: usize,
    /// Last seen connection status (WS).
    pub connection_status: ConnectionStatus,
    /// Whether the stream should render in tiles or list layout.
    pub layout: StreamLayout,
    /// Filter applied to the stream view only.
    pub filter: Filter,
    /// Current filter input buffer when `/` is active.
    pub filter_input: String,
    /// Text being typed while answering a question.
    pub answer_buffer: String,
    /// Current input mode in the UI.
    pub input_mode: InputMode,
    /// Whether the standalone queue tab is visible.
    pub queue_tab: bool,
    /// Whether the help overlay is visible.
    pub help_visible: bool,
    /// Whether the UI should exit.
    pub should_exit: bool,
    /// Timestamp until which the flash effect remains active.
    flash_until: Option<Instant>,
    /// Pending queue IDs for deduplication.
    pending_queue_ids: HashSet<Uuid>,
}

impl MonitorState {
    /// Create initial monitor state with a default connection status.
    pub fn new(connection_status: ConnectionStatus) -> Self {
        MonitorState {
            channels: HashMap::new(),
            queue: VecDeque::new(),
            queue_index: 0,
            connection_status,
            layout: StreamLayout::Tiles,
            filter: Filter::default(),
            filter_input: String::new(),
            answer_buffer: String::new(),
            input_mode: InputMode::Normal,
            queue_tab: false,
            help_visible: false,
            flash_until: None,
            pending_queue_ids: HashSet::new(),
            should_exit: false,
        }
    }

    /// Apply an incoming event from the networking layer.
    pub fn apply_event(&mut self, event: MonitorEvent) {
        match event {
            MonitorEvent::ConnectionStatus(status) => {
                self.connection_status = status;
            }
            MonitorEvent::Message(message) => {
                self.last_seen_message(message);
            }
        }
    }

    fn last_seen_message(&mut self, message: MonitorMessage) {
        let now = Utc::now();
        self.insert_channel_message(message.clone());

        if message.is_blocking() && !self.pending_queue_ids.contains(&message.id) {
            self.pending_queue_ids.insert(message.id);
            self.queue.push_back(QueueItem::new(message.clone()));
            self.ensure_queue_index();
            self.trigger_flash();
            self.move_focus_to_new();
        }

        if let Some(correlation) = message.correlation_id {
            self.remove_queue_item(&correlation);
        }

        let mut flash = false;
        for item in &mut self.queue {
            if item.check_near_timeout(now) {
                flash = true;
            }
        }
        if flash {
            self.trigger_flash();
        }
    }

    fn insert_channel_message(&mut self, message: MonitorMessage) {
        let key = message.channel.clone();
        let channel = self.channels.entry(key.clone()).or_insert_with(|| {
            let (project, branch) = split_channel(&key);
            ChannelState::new(project, branch)
        });
        channel.push(message);
    }

    fn move_focus_to_new(&mut self) {
        self.queue_index = self.queue.len().saturating_sub(1);
    }

    fn ensure_queue_index(&mut self) {
        if self.queue.is_empty() {
            self.queue_index = 0;
        } else if self.queue_index >= self.queue.len() {
            self.queue_index = self.queue.len() - 1;
        }
    }

    fn remove_queue_item(&mut self, correlation_id: &Uuid) {
        if let Some(position) = self
            .queue
            .iter()
            .position(|item| item.message.id == *correlation_id)
        {
            let removed = self.queue.remove(position);
            if let Some(item) = removed {
                self.pending_queue_ids.remove(&item.message.id);
            }
            self.ensure_queue_index();
        }
    }

    fn trigger_flash(&mut self) {
        self.flash_until = Some(Instant::now() + FLASH_DURATION);
    }

    /// Whether the flash indicator is currently visible.
    pub fn flash_active(&self) -> bool {
        self.flash_until
            .map(|deadline| Instant::now() <= deadline)
            .unwrap_or(false)
    }

    /// Advance to the next queue item (wrap-around).
    pub fn next_queue(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        self.queue_index = (self.queue_index + 1) % self.queue.len();
    }

    /// Move focus to the previous queue item.
    pub fn previous_queue(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        if self.queue_index == 0 {
            self.queue_index = self.queue.len() - 1;
        } else {
            self.queue_index -= 1;
        }
    }

    /// Toggle between tiles and list layout.
    pub fn toggle_layout(&mut self) {
        self.layout = match self.layout {
            StreamLayout::Tiles => StreamLayout::List,
            StreamLayout::List => StreamLayout::Tiles,
        };
    }

    /// Set the filter text (parsed as `project` or `project/branch`).
    pub fn apply_filter(&mut self, raw: &str) {
        self.filter.parse(raw);
    }

    /// Clear the current filter.
    pub fn clear_filter(&mut self) {
        self.filter.clear();
    }

    /// Show the queue tab overlay.
    pub fn toggle_queue_tab(&mut self) {
        self.queue_tab = !self.queue_tab;
    }

    /// Select the focused queue item (if any).
    pub fn selected_queue(&self) -> Option<&QueueItem> {
        self.queue.get(self.queue_index)
    }

    /// Update near-timeout tracking and flash timers.
    pub fn tick(&mut self) {
        let now = Utc::now();
        let mut flash = false;
        for item in &mut self.queue {
            if item.check_near_timeout(now) {
                flash = true;
            }
        }
        if flash {
            self.trigger_flash();
        }
        if let Some(deadline) = self.flash_until {
            if Instant::now() > deadline {
                self.flash_until = None;
            }
        }
    }

    /// Mark the UI as ready to exit.
    pub fn request_exit(&mut self) {
        self.should_exit = true;
    }

    /// Check if the UI loop should stop.
    pub fn exit_requested(&self) -> bool {
        self.should_exit
    }
}

/// A channel grouping that holds the latest messages.
pub struct ChannelState {
    pub project: String,
    pub branch: String,
    pub messages: VecDeque<MonitorMessage>,
}

impl ChannelState {
    fn new(project: String, branch: String) -> Self {
        ChannelState {
            project,
            branch,
            messages: VecDeque::new(),
        }
    }

    fn push(&mut self, item: MonitorMessage) {
        if self.messages.len() >= CHANNEL_BUFFER_CAPACITY {
            self.messages.pop_front();
        }
        self.messages.push_back(item);
    }
}

/// An outstanding blocking item shown in the queue.
pub struct QueueItem {
    pub message: MonitorMessage,
    flash_until: Option<Instant>,
    alerted_near_timeout: bool,
}

impl QueueItem {
    fn new(message: MonitorMessage) -> Self {
        QueueItem {
            message,
            flash_until: Some(Instant::now() + FLASH_DURATION),
            alerted_near_timeout: false,
        }
    }

    /// Whether the near-timeout threshold has just been breached.
    fn check_near_timeout(&mut self, now: DateTime<Utc>) -> bool {
        if self.alerted_near_timeout {
            return false;
        }
        if let Some(timeout) = self.message.timeout_seconds {
            let elapsed = now.signed_duration_since(self.message.timestamp);
            if elapsed.num_seconds() < 0 {
                return false;
            }
            let elapsed = match elapsed.to_std() {
                Ok(dur) => dur,
                Err(_) => return false,
            };
            let timeout_dur = StdDuration::from_secs(timeout);
            if elapsed >= timeout_dur {
                return false;
            }
            let remaining = timeout_dur
                .checked_sub(elapsed)
                .unwrap_or_else(|| StdDuration::from_secs(0));
            let threshold = near_timeout_threshold(timeout_dur);
            if remaining <= threshold {
                self.alerted_near_timeout = true;
                return true;
            }
        }
        false
    }

    /// Whether this queue item currently has the flash flag.
    pub fn is_flashing(&self) -> bool {
        self.flash_until
            .map(|deadline| Instant::now() <= deadline)
            .unwrap_or(false)
    }
}

fn near_timeout_threshold(timeout: StdDuration) -> StdDuration {
    let ten_percent = StdDuration::from_secs((timeout.as_secs_f64() * 0.1).ceil() as u64);
    std::cmp::min(
        StdDuration::from_secs(30),
        ten_percent.max(StdDuration::from_secs(1)),
    )
}

/// Layout mode for the stream view.
#[derive(Debug, Copy, Clone)]
pub enum StreamLayout {
    Tiles,
    List,
}

/// Parsed filter that matches project/branch names.
#[derive(Debug, Default)]
pub struct Filter {
    project: Option<String>,
    branch: Option<String>,
}

impl Filter {
    pub fn parse(&mut self, raw: &str) {
        let raw = raw.trim();
        if raw.is_empty() {
            self.clear();
            return;
        }
        if let Some((project, branch)) = raw.split_once('/') {
            self.project = Some(project.trim().to_string());
            self.branch = Some(branch.trim().to_string());
        } else {
            self.project = Some(raw.to_string());
            self.branch = None;
        }
    }

    pub fn clear(&mut self) {
        self.project = None;
        self.branch = None;
    }

    pub fn matches(&self, project: &str, branch: &str) -> bool {
        if let Some(ref project_filter) = self.project {
            if project_filter != project {
                return false;
            }
        }
        if let Some(ref branch_filter) = self.branch {
            if branch_filter != branch {
                return false;
            }
        }
        true
    }

    pub fn display(&self) -> String {
        match (&self.project, &self.branch) {
            (Some(project), Some(branch)) => format!("{}/{}", project, branch),
            (Some(project), None) => project.clone(),
            _ => String::new(),
        }
    }
}

/// Input mode for the TUI.
#[derive(Debug, Clone)]
pub enum InputMode {
    Normal,
    Filter,
    Answer { target: Uuid },
    Authorization { target: Uuid },
}

fn split_channel(channel: &str) -> (String, String) {
    if let Some(pos) = channel.find('/') {
        (channel[..pos].to_string(), channel[pos + 1..].to_string())
    } else {
        ("uncategorized".to_string(), channel.to_string())
    }
}
