use super::*;

impl FrameRoot {
    pub(super) fn selected_source_metadata_entry(&self) -> SourceMetadataEntry {
        self.source_metadata.selected_entry(&self.file_queue)
    }

    pub(super) fn selected_source_metadata(&self) -> Option<SourceMetadata> {
        self.file_queue
            .selected_file_id()
            .and_then(|id| self.source_metadata.metadata_for(id))
            .cloned()
    }

    #[must_use]
    pub(super) fn selected_bitrate_analysis_entry(&self) -> BitrateAnalysisEntry {
        self.file_queue.selected_file_id().map_or_else(
            BitrateAnalysisEntry::default,
            |id| self.bitrate_analysis.entry_for(id),
        )
    }

    pub(super) fn set_source_info_view(
        &mut self,
        view: SourceInfoView,
        cx: &mut Context<Self>,
    ) {
        self.settings_ui.source_info_view = view;
        if view == SourceInfoView::BitrateAnalysis {
            self.ensure_bitrate_analysis_for_selection(cx);
        }
        cx.notify();
    }

    pub(super) fn set_bitrate_window(&mut self, window_s: f64, cx: &mut Context<Self>) {
        self.settings_ui.bitrate_window_s = window_s;
        self.settings_ui.bitrate_curve_hover = None;
        cx.notify();
    }

    /// Updates the bitrate-curve hover index. Called from per-column
    /// `.on_hover` listeners on the curve; the value is pure view state and
    /// not persisted.
    pub(super) fn set_bitrate_curve_hover(
        &mut self,
        column_index: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        if self.settings_ui.bitrate_curve_hover != column_index {
            self.settings_ui.bitrate_curve_hover = column_index;
            cx.notify();
        }
    }

    /// Toggles the bitrate-window dropdown popover. Mutually exclusive with
    /// video selects and the preset menu (they get hidden on open).
    pub(super) fn toggle_bitrate_window_popover(&mut self, cx: &mut Context<Self>) {
        if self.settings_ui.bitrate_window_popover.is_open() {
            self.settings_ui.bitrate_window_popover = PopoverState::Hidden;
        } else {
            self.close_video_selects_immediate();
            self.close_preset_menu();
            self.settings_ui.bitrate_window_popover = PopoverState::Open;
        }
        cx.notify();
    }

    pub(super) fn close_bitrate_window_popover(&mut self) {
        self.settings_ui.bitrate_window_popover = PopoverState::Hidden;
    }

    /// Runs the bundled-ffprobe bitrate analysis once per file, on demand. Idle
    /// and Error entries re-run (re-entering the view retries); Loading/Ready
    /// are left untouched.
    fn ensure_bitrate_analysis_for_selection(&mut self, cx: &mut Context<Self>) {
        let Some(file_id) = self.file_queue.selected_file_id().map(str::to_string) else {
            return;
        };
        if !matches!(
            self.bitrate_analysis.entry_for(&file_id).status,
            BitrateAnalysisStatus::Idle | BitrateAnalysisStatus::Error
        ) {
            return;
        }
        let Some(metadata) = self.source_metadata.metadata_for(&file_id).cloned() else {
            return;
        };
        let Some(stream_index) = metadata.video_stream_index else {
            return;
        };
        let Some(video_codec) = metadata.video_codec else {
            return;
        };
        let Some(file_path) = self
            .file_queue
            .file_by_id(&file_id)
            .map(|file| file.path.clone())
        else {
            return;
        };

        self.bitrate_analysis.mark_loading(file_id.clone());
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    run_bitrate_analysis(&file_path, stream_index, &video_codec)
                })
                .await;

            this.update(cx, |root, cx| {
                match result {
                    Ok(data) => root.bitrate_analysis.mark_ready(
                        file_id.clone(),
                        std::sync::Arc::new(data),
                    ),
                    Err(error) => root
                        .bitrate_analysis
                        .mark_error(file_id.clone(), error.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn queue_source_metadata_probe(
        &mut self,
        file_id: String,
        file_path: String,
        cx: &mut Context<Self>,
    ) {
        self.queue_source_metadata_probe_inner(file_id, file_path, true, cx);
    }

    pub(super) fn queue_restored_source_metadata_probe(
        &mut self,
        file_id: String,
        file_path: String,
        cx: &mut Context<Self>,
    ) {
        self.queue_source_metadata_probe_inner(file_id, file_path, false, cx);
    }

    fn queue_source_metadata_probe_inner(
        &mut self,
        file_id: String,
        file_path: String,
        normalize_selected_config: bool,
        cx: &mut Context<Self>,
    ) {
        self.source_metadata.mark_loading(file_id.clone());
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { probe_source_metadata(&file_path) })
                .await;

            this.update(cx, |root, cx| {
                match result {
                    Ok(metadata) => {
                        if normalize_selected_config
                            && !root.update_installation_in_progress()
                            && let Some(file) = root.file_queue.file_by_id_mut(&file_id)
                        {
                            initialize_output_config(&mut file.config, Some(&metadata));
                        }
                        root.source_metadata.mark_ready(file_id.clone(), metadata);
                        if root.file_queue.selected_file_id() == Some(file_id.as_str()) {
                            let selected_metadata = root.selected_source_metadata();
                            root.resolve_selected_settings_tab(selected_metadata.as_ref());
                        }
                    }
                    Err(error) => {
                        root.source_metadata
                            .mark_error(file_id.clone(), error.to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
