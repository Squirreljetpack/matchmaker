use log::{error};
use ratatui::{
    layout::Rect,
    widgets::{Paragraph, Wrap},
};

use crate::{
    config::{PreviewConfig, PreviewLayoutSetting},
    spawn::preview::PreviewerView,
};

#[derive(Debug)]
pub struct PreviewUI {
    pub view: PreviewerView,
    config: PreviewConfig,
    pub layout_idx: usize,
    pub area: Rect,
    pub offset: u16,
}

impl PreviewUI {
    pub fn new(view: PreviewerView, config: PreviewConfig) -> Self {
        Self {
            view,
            config,
            layout_idx: 0,
            offset: 0,
            area: Rect::default()
        }
    }
    
    pub fn is_show(&self) -> bool {
        self.config.show && self.layout().max != 0 // sentinel for hidden preview
        // btw, for running background tasks, use an event handler
    }
    pub fn show<const SHOW: bool>(&mut self) -> bool {
        let previous = self.config.show;
        self.config.show = SHOW;
        previous != SHOW
    }
    pub fn toggle_show(&mut self) {
        self.config.show = !self.config.show;
    }
    
    pub fn layout(&self) -> &PreviewLayoutSetting {
        &self.config.layout[self.layout_idx].layout
    }
    pub fn command(&self) -> &String {
        &self.config.layout[self.layout_idx].command
    }
    
    // pub fn up(&mut self, n: u16) {
    //     if self.offset >= n {
    //         self.offset -= n;
    //     } else {
    //         self.offset = 0;
    //     }
    // }
    // pub fn down(&mut self, n: u16) {
    //     let total_lines = self.view.len() as u16;
    //     self.offset = (total_lines + self.area.height).min(self.offset + n);
    // }
    
    pub fn up(&mut self, n: u16) {
        if self.offset >= n {
            self.offset -= n;
        } else if self.config.scroll_wrap {
            let total_lines = self.view.len() as u16;
            self.offset = total_lines.saturating_sub(n - self.offset);
        } else {
            self.offset = 0;
        }
    }
    
    pub fn down(&mut self, n: u16) {
        let total_lines = self.view.len() as u16;
        
        if self.offset + n > total_lines {
            if self.config.scroll_wrap {
                self.offset = 0;
            } else {
                self.offset = total_lines;
            }
        } else {
            self.offset += n;
        }
    }
    
    
    pub fn update_dimensions(&mut self, area: &Rect) {
        let mut height = area.height;
        height -= self.config.border.height();
        self.area.height = height;
        
        let mut width = area.width;
        width -= self.config.border.width();
        self.area.width = width;
    }
    
    pub fn cycle_layout(&mut self) {
        self.layout_idx = (self.layout_idx + 1) % self.config.layout.len()
    }
    
    
    pub fn set_idx(&mut self, idx: u8) -> bool {
        let idx = idx as usize;
        if idx <= self.config.layout.len() {
            let changed = self.layout_idx != idx;
            self.layout_idx = idx;
            changed
        } else {
            error!("Layout idx {idx} out of bounds, ignoring.");
            false
        }
    }
    
    pub fn make_preview(&self) -> Paragraph<'_> {
        let results = self.view.results();
        let height = self.area.height as usize;
        let offset = self.offset as usize;
        
        // todo: can we avoid cloning?
        let visible_lines: Vec<_> = results
        .iter()
        .skip(offset)
        .take(height)
        .cloned()
        .collect();
        
        let mut preview = Paragraph::new(visible_lines);
        preview = preview.block(self.config.border.as_block());
        if self.config.wrap {
            preview = preview.wrap(Wrap { trim: true });
        }
        preview
    }
}
