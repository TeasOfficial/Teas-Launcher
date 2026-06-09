//! 下载任务追踪器 — 参照 PCL-CE ModLoader.loaderTaskbar
//!
//! 全局管理下载任务的状态、进度和历史记录
//! 该模块为 PCL-CE 完整移植，前端当前未直接调用，标记允许死代码
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use once_cell::sync::Lazy;

/// 全局下载任务追踪器
static TRACKER: Lazy<Mutex<DownloadTracker>> =
    Lazy::new(|| Mutex::new(DownloadTracker::new()));

/// 单个任务追踪信息
#[derive(Debug, Clone)]
pub struct TaskInfo {
    pub id: String,
    pub name: String,
    pub status: TaskStatus,
    pub total_files: u64,
    pub completed_files: u64,
    pub failed_files: u64,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub speed: u64,
    pub start_time: Instant,
    pub finish_time: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TaskStatus {
    Waiting,
    Running,
    Finished,
    Failed,
    Cancelled,
}

/// 全局下载任务管理器
pub struct DownloadTracker {
    tasks: HashMap<String, TaskInfo>,
    history: Vec<TaskInfo>,
    next_id: u64,
}

impl DownloadTracker {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            history: Vec::new(),
            next_id: 0,
        }
    }

    /// 注册新任务
    pub fn register(&mut self, name: String) -> String {
        let id = format!("dl_{}", self.next_id);
        self.next_id += 1;
        let task = TaskInfo {
            id: id.clone(),
            name,
            status: TaskStatus::Waiting,
            total_files: 0,
            completed_files: 0,
            failed_files: 0,
            total_bytes: 0,
            downloaded_bytes: 0,
            speed: 0,
            start_time: Instant::now(),
            finish_time: None,
        };
        self.tasks.insert(id.clone(), task);
        id
    }

    /// 更新任务状态
    pub fn update_status(&mut self, id: &str, status: TaskStatus) {
        if let Some(task) = self.tasks.get_mut(id) {
            task.status = status;
            if status == TaskStatus::Finished || status == TaskStatus::Failed || status == TaskStatus::Cancelled {
                task.finish_time = Some(Instant::now());
            }
        }
    }

    /// 更新任务进度
    pub fn update_progress(
        &mut self,
        id: &str,
        total: u64,
        completed: u64,
        failed: u64,
        speed: u64,
    ) {
        if let Some(task) = self.tasks.get_mut(id) {
            task.total_files = total;
            task.completed_files = completed;
            task.failed_files = failed;
            task.speed = speed;
        }
    }

    /// 完成一个文件
    pub fn file_done(&mut self, id: &str, success: bool) {
        if let Some(task) = self.tasks.get_mut(id) {
            if success {
                task.completed_files += 1;
            } else {
                task.failed_files += 1;
            }
        }
    }

    /// 完成任务 → 移入历史
    pub fn finish(&mut self, id: &str) {
        if let Some(task) = self.tasks.remove(id) {
            let mut finished_task = task;
            finished_task.finish_time = Some(Instant::now());
            self.history.push(finished_task);
        }
    }

    /// 获取任务信息
    pub fn get(&self, id: &str) -> Option<&TaskInfo> {
        self.tasks.get(id)
    }

    /// 获取所有活跃任务
    pub fn active_tasks(&self) -> Vec<&TaskInfo> {
        self.tasks.values().collect()
    }

    /// 获取历史记录
    pub fn history(&self) -> &[TaskInfo] {
        &self.history
    }
}

/// 全局注册任务
pub fn register_task(name: String) -> String {
    TRACKER.lock().unwrap().register(name)
}

/// 全局更新状态
pub fn update_task_status(id: &str, status: TaskStatus) {
    TRACKER.lock().unwrap().update_status(id, status);
}

/// 全局完成任务
pub fn finish_task(id: &str) {
    TRACKER.lock().unwrap().finish(id);
}

/// 获取活跃任务数
pub fn active_count() -> usize {
    TRACKER.lock().unwrap().tasks.len()
}
