use super::{
    Bounds, FocusHandle, Pixels, Range, SETTINGS_CONTROL_HEIGHT, ShapedLine,
    TEXT_INPUT_CARET_BASE_HEIGHT, TEXT_INPUT_CARET_WIDTH, Task,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::app) struct FrameTextInputMetrics {
    pub(in crate::app) control_height: f32,
    pub(in crate::app) caret_height: f32,
    pub(in crate::app) caret_width: f32,
}

impl FrameTextInputMetrics {
    pub(in crate::app) fn from_ui_scale(ui_scale: f32) -> Self {
        Self {
            control_height: SETTINGS_CONTROL_HEIGHT * ui_scale,
            caret_height: TEXT_INPUT_CARET_BASE_HEIGHT * ui_scale,
            caret_width: TEXT_INPUT_CARET_WIDTH * ui_scale,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) enum FrameTextInputKind {
    MaxConcurrency,
    OutputName,
    AudioBitrate,
    VideoCustomWidth,
    VideoCustomHeight,
    VideoBitrate,
    VideoMaxrate,
    VideoBufsize,
    VideoX264PsyRd,
    VideoX265PsyRd,
    VideoX265PsyRdoq,
    GifLoop,
    PreviewStartTime,
    PreviewEndTime,
    MetadataTitle,
    MetadataArtist,
    MetadataAlbum,
    MetadataGenre,
    MetadataDate,
    MetadataComment,
    MetadataServiceName,
    MetadataServiceProvider,
    PresetName,
    ExternalSubtitleLanguage,
    ExternalSubtitleTitle,
    SubtitleFontColorHex,
    SubtitleOutlineColorHex,
}

impl FrameTextInputKind {
    pub(in crate::app) const ALL: [Self; 27] = [
        Self::MaxConcurrency,
        Self::OutputName,
        Self::AudioBitrate,
        Self::VideoCustomWidth,
        Self::VideoCustomHeight,
        Self::VideoBitrate,
        Self::VideoMaxrate,
        Self::VideoBufsize,
        Self::VideoX264PsyRd,
        Self::VideoX265PsyRd,
        Self::VideoX265PsyRdoq,
        Self::GifLoop,
        Self::PreviewStartTime,
        Self::PreviewEndTime,
        Self::MetadataTitle,
        Self::MetadataArtist,
        Self::MetadataAlbum,
        Self::MetadataGenre,
        Self::MetadataDate,
        Self::MetadataComment,
        Self::MetadataServiceName,
        Self::MetadataServiceProvider,
        Self::PresetName,
        Self::ExternalSubtitleLanguage,
        Self::ExternalSubtitleTitle,
        Self::SubtitleFontColorHex,
        Self::SubtitleOutlineColorHex,
    ];

    pub(in crate::app) const fn accessibility_label(self) -> &'static str {
        match self {
            Self::MaxConcurrency => "最大并发数",
            Self::OutputName => "输出名称",
            Self::AudioBitrate => "音频码率",
            Self::VideoCustomWidth => "视频宽度",
            Self::VideoCustomHeight => "视频高度",
            Self::VideoBitrate => "视频码率",
            Self::VideoMaxrate => "视频最大码率",
            Self::VideoBufsize => "视频 VBV 缓冲大小",
            Self::VideoX264PsyRd => "H.264 心理视觉强度",
            Self::VideoX265PsyRd => "H.265 心理视觉强度",
            Self::VideoX265PsyRdoq => "H.265 心理视觉量化",
            Self::GifLoop => "GIF 循环次数",
            Self::PreviewStartTime => "预览开始时间",
            Self::PreviewEndTime => "预览结束时间",
            Self::MetadataTitle => "元数据标题",
            Self::MetadataArtist => "元数据艺术家",
            Self::MetadataAlbum => "元数据专辑",
            Self::MetadataGenre => "元数据流派",
            Self::MetadataDate => "元数据日期 / 年份",
            Self::MetadataComment => "元数据注释",
            Self::MetadataServiceName => "MPEG 传输流服务名称",
            Self::MetadataServiceProvider => "MPEG 传输流服务提供商",
            Self::PresetName => "预设名称",
            Self::ExternalSubtitleLanguage => "可选字幕语言",
            Self::ExternalSubtitleTitle => "可选字幕标题",
            Self::SubtitleFontColorHex => "字幕字体颜色",
            Self::SubtitleOutlineColorHex => "字幕描边颜色",
        }
    }

    pub(in crate::app) const fn is_preview_timecode(self) -> bool {
        matches!(self, Self::PreviewStartTime | Self::PreviewEndTime)
    }

    /// 元数据页的可编辑字段（含 MPEG-TS 的服务名称/服务提供商），用于把占位提示限定为斜体。
    pub(in crate::app) const fn is_metadata_field(self) -> bool {
        matches!(
            self,
            Self::MetadataTitle
                | Self::MetadataArtist
                | Self::MetadataAlbum
                | Self::MetadataGenre
                | Self::MetadataDate
                | Self::MetadataComment
                | Self::MetadataServiceName
                | Self::MetadataServiceProvider
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::app) struct FrameTimecodeInputDraft {
    pub(in crate::app) file_id: String,
    pub(in crate::app) value: String,
}

pub(in crate::app) struct FrameTextInputRuntime {
    pub(in crate::app) selected_range: Range<usize>,
    pub(in crate::app) selection_reversed: bool,
    pub(in crate::app) marked_range: Option<Range<usize>>,
    pub(in crate::app) last_layout: Option<ShapedLine>,
    pub(in crate::app) last_bounds: Option<Bounds<Pixels>>,
    pub(in crate::app) scroll_x: Pixels,
    pub(in crate::app) is_selecting: bool,
    pub(in crate::app) timecode_draft: Option<FrameTimecodeInputDraft>,
}

impl Default for FrameTextInputRuntime {
    fn default() -> Self {
        Self {
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            scroll_x: Pixels::ZERO,
            is_selecting: false,
            timecode_draft: None,
        }
    }
}

pub(in crate::app) fn clamp_text_input_scroll_x(
    scroll_x: Pixels,
    content_width: Pixels,
    viewport_width: Pixels,
) -> Pixels {
    scroll_x.clamp(
        Pixels::ZERO,
        (content_width - viewport_width).max(Pixels::ZERO),
    )
}

pub(in crate::app) fn text_input_scroll_x_for_cursor(
    current: Pixels,
    cursor_x: Pixels,
    content_width: Pixels,
    viewport_width: Pixels,
    caret_width: Pixels,
) -> Pixels {
    let mut next = current;
    let trailing_edge = (viewport_width - caret_width).max(Pixels::ZERO);

    if cursor_x - next > trailing_edge {
        next = cursor_x - trailing_edge;
    } else if cursor_x < next {
        next = cursor_x;
    }

    clamp_text_input_scroll_x(next, content_width, viewport_width)
}

#[cfg(test)]
mod metrics_tests {
    #![expect(
        clippy::float_cmp,
        reason = "Input metrics tests compare exact deterministic scale products."
    )]

    use super::*;

    #[test]
    fn input_metrics_preserve_baseline_dimensions() {
        let metrics = FrameTextInputMetrics::from_ui_scale(1.0);
        assert_eq!(metrics.control_height, SETTINGS_CONTROL_HEIGHT);
        assert_eq!(metrics.caret_height, TEXT_INPUT_CARET_BASE_HEIGHT);
        assert_eq!(metrics.caret_width, TEXT_INPUT_CARET_WIDTH);
    }

    #[test]
    fn ui_scale_resizes_control_and_caret_together() {
        let metrics = FrameTextInputMetrics::from_ui_scale(2.0);
        assert_eq!(metrics.control_height, 60.0);
        assert_eq!(metrics.caret_height, 28.0);
        assert_eq!(metrics.caret_width, 3.0);
    }
}

#[derive(Default)]
pub(in crate::app) struct FrameTextInputStore {
    max_concurrency: FrameTextInputRuntime,
    output_name: FrameTextInputRuntime,
    audio_bitrate: FrameTextInputRuntime,
    video_width: FrameTextInputRuntime,
    video_height: FrameTextInputRuntime,
    video_bitrate: FrameTextInputRuntime,
    video_maxrate: FrameTextInputRuntime,
    video_bufsize: FrameTextInputRuntime,
    video_x264_psy_rd: FrameTextInputRuntime,
    video_x265_psy_rd: FrameTextInputRuntime,
    video_x265_psy_rdoq: FrameTextInputRuntime,
    gif_loop: FrameTextInputRuntime,
    preview_start_time: FrameTextInputRuntime,
    preview_end_time: FrameTextInputRuntime,
    metadata_title: FrameTextInputRuntime,
    metadata_artist: FrameTextInputRuntime,
    metadata_album: FrameTextInputRuntime,
    metadata_genre: FrameTextInputRuntime,
    metadata_date: FrameTextInputRuntime,
    metadata_comment: FrameTextInputRuntime,
    metadata_service_name: FrameTextInputRuntime,
    metadata_service_provider: FrameTextInputRuntime,
    preset_name: FrameTextInputRuntime,
    external_subtitle_language: FrameTextInputRuntime,
    external_subtitle_title: FrameTextInputRuntime,
    subtitle_font_color: FrameTextInputRuntime,
    subtitle_outline_color: FrameTextInputRuntime,
}

impl FrameTextInputStore {
    pub(in crate::app) const fn runtime(&self, kind: FrameTextInputKind) -> &FrameTextInputRuntime {
        match kind {
            FrameTextInputKind::MaxConcurrency => &self.max_concurrency,
            FrameTextInputKind::OutputName => &self.output_name,
            FrameTextInputKind::AudioBitrate => &self.audio_bitrate,
            FrameTextInputKind::VideoCustomWidth => &self.video_width,
            FrameTextInputKind::VideoCustomHeight => &self.video_height,
            FrameTextInputKind::VideoBitrate => &self.video_bitrate,
            FrameTextInputKind::VideoMaxrate => &self.video_maxrate,
            FrameTextInputKind::VideoBufsize => &self.video_bufsize,
            FrameTextInputKind::VideoX264PsyRd => &self.video_x264_psy_rd,
            FrameTextInputKind::VideoX265PsyRd => &self.video_x265_psy_rd,
            FrameTextInputKind::VideoX265PsyRdoq => &self.video_x265_psy_rdoq,
            FrameTextInputKind::GifLoop => &self.gif_loop,
            FrameTextInputKind::PreviewStartTime => &self.preview_start_time,
            FrameTextInputKind::PreviewEndTime => &self.preview_end_time,
            FrameTextInputKind::MetadataTitle => &self.metadata_title,
            FrameTextInputKind::MetadataArtist => &self.metadata_artist,
            FrameTextInputKind::MetadataAlbum => &self.metadata_album,
            FrameTextInputKind::MetadataGenre => &self.metadata_genre,
            FrameTextInputKind::MetadataDate => &self.metadata_date,
            FrameTextInputKind::MetadataComment => &self.metadata_comment,
            FrameTextInputKind::MetadataServiceName => &self.metadata_service_name,
            FrameTextInputKind::MetadataServiceProvider => &self.metadata_service_provider,
            FrameTextInputKind::PresetName => &self.preset_name,
            FrameTextInputKind::ExternalSubtitleLanguage => &self.external_subtitle_language,
            FrameTextInputKind::ExternalSubtitleTitle => &self.external_subtitle_title,
            FrameTextInputKind::SubtitleFontColorHex => &self.subtitle_font_color,
            FrameTextInputKind::SubtitleOutlineColorHex => &self.subtitle_outline_color,
        }
    }

    pub(in crate::app) const fn runtime_mut(
        &mut self,
        kind: FrameTextInputKind,
    ) -> &mut FrameTextInputRuntime {
        match kind {
            FrameTextInputKind::MaxConcurrency => &mut self.max_concurrency,
            FrameTextInputKind::OutputName => &mut self.output_name,
            FrameTextInputKind::AudioBitrate => &mut self.audio_bitrate,
            FrameTextInputKind::VideoCustomWidth => &mut self.video_width,
            FrameTextInputKind::VideoCustomHeight => &mut self.video_height,
            FrameTextInputKind::VideoBitrate => &mut self.video_bitrate,
            FrameTextInputKind::VideoMaxrate => &mut self.video_maxrate,
            FrameTextInputKind::VideoBufsize => &mut self.video_bufsize,
            FrameTextInputKind::VideoX264PsyRd => &mut self.video_x264_psy_rd,
            FrameTextInputKind::VideoX265PsyRd => &mut self.video_x265_psy_rd,
            FrameTextInputKind::VideoX265PsyRdoq => &mut self.video_x265_psy_rdoq,
            FrameTextInputKind::GifLoop => &mut self.gif_loop,
            FrameTextInputKind::PreviewStartTime => &mut self.preview_start_time,
            FrameTextInputKind::PreviewEndTime => &mut self.preview_end_time,
            FrameTextInputKind::MetadataTitle => &mut self.metadata_title,
            FrameTextInputKind::MetadataArtist => &mut self.metadata_artist,
            FrameTextInputKind::MetadataAlbum => &mut self.metadata_album,
            FrameTextInputKind::MetadataGenre => &mut self.metadata_genre,
            FrameTextInputKind::MetadataDate => &mut self.metadata_date,
            FrameTextInputKind::MetadataComment => &mut self.metadata_comment,
            FrameTextInputKind::MetadataServiceName => &mut self.metadata_service_name,
            FrameTextInputKind::MetadataServiceProvider => &mut self.metadata_service_provider,
            FrameTextInputKind::PresetName => &mut self.preset_name,
            FrameTextInputKind::ExternalSubtitleLanguage => &mut self.external_subtitle_language,
            FrameTextInputKind::ExternalSubtitleTitle => &mut self.external_subtitle_title,
            FrameTextInputKind::SubtitleFontColorHex => &mut self.subtitle_font_color,
            FrameTextInputKind::SubtitleOutlineColorHex => &mut self.subtitle_outline_color,
        }
    }
}

#[derive(Default)]
pub(in crate::app) struct FrameTextInputFocusStore {
    max_concurrency: Option<FocusHandle>,
    output_name: Option<FocusHandle>,
    audio_bitrate: Option<FocusHandle>,
    video_width: Option<FocusHandle>,
    video_height: Option<FocusHandle>,
    video_bitrate: Option<FocusHandle>,
    video_maxrate: Option<FocusHandle>,
    video_bufsize: Option<FocusHandle>,
    video_x264_psy_rd: Option<FocusHandle>,
    video_x265_psy_rd: Option<FocusHandle>,
    video_x265_psy_rdoq: Option<FocusHandle>,
    gif_loop: Option<FocusHandle>,
    preview_start_time: Option<FocusHandle>,
    preview_end_time: Option<FocusHandle>,
    metadata_title: Option<FocusHandle>,
    metadata_artist: Option<FocusHandle>,
    metadata_album: Option<FocusHandle>,
    metadata_genre: Option<FocusHandle>,
    metadata_date: Option<FocusHandle>,
    metadata_comment: Option<FocusHandle>,
    metadata_service_name: Option<FocusHandle>,
    metadata_service_provider: Option<FocusHandle>,
    preset_name: Option<FocusHandle>,
    external_subtitle_language: Option<FocusHandle>,
    external_subtitle_title: Option<FocusHandle>,
    subtitle_font_color: Option<FocusHandle>,
    subtitle_outline_color: Option<FocusHandle>,
}

impl FrameTextInputFocusStore {
    pub(in crate::app) const fn focus(&self, kind: FrameTextInputKind) -> Option<&FocusHandle> {
        match kind {
            FrameTextInputKind::MaxConcurrency => self.max_concurrency.as_ref(),
            FrameTextInputKind::OutputName => self.output_name.as_ref(),
            FrameTextInputKind::AudioBitrate => self.audio_bitrate.as_ref(),
            FrameTextInputKind::VideoCustomWidth => self.video_width.as_ref(),
            FrameTextInputKind::VideoCustomHeight => self.video_height.as_ref(),
            FrameTextInputKind::VideoBitrate => self.video_bitrate.as_ref(),
            FrameTextInputKind::VideoMaxrate => self.video_maxrate.as_ref(),
            FrameTextInputKind::VideoBufsize => self.video_bufsize.as_ref(),
            FrameTextInputKind::VideoX264PsyRd => self.video_x264_psy_rd.as_ref(),
            FrameTextInputKind::VideoX265PsyRd => self.video_x265_psy_rd.as_ref(),
            FrameTextInputKind::VideoX265PsyRdoq => self.video_x265_psy_rdoq.as_ref(),
            FrameTextInputKind::GifLoop => self.gif_loop.as_ref(),
            FrameTextInputKind::PreviewStartTime => self.preview_start_time.as_ref(),
            FrameTextInputKind::PreviewEndTime => self.preview_end_time.as_ref(),
            FrameTextInputKind::MetadataTitle => self.metadata_title.as_ref(),
            FrameTextInputKind::MetadataArtist => self.metadata_artist.as_ref(),
            FrameTextInputKind::MetadataAlbum => self.metadata_album.as_ref(),
            FrameTextInputKind::MetadataGenre => self.metadata_genre.as_ref(),
            FrameTextInputKind::MetadataDate => self.metadata_date.as_ref(),
            FrameTextInputKind::MetadataComment => self.metadata_comment.as_ref(),
            FrameTextInputKind::MetadataServiceName => self.metadata_service_name.as_ref(),
            FrameTextInputKind::MetadataServiceProvider => self.metadata_service_provider.as_ref(),
            FrameTextInputKind::PresetName => self.preset_name.as_ref(),
            FrameTextInputKind::ExternalSubtitleLanguage => {
                self.external_subtitle_language.as_ref()
            }
            FrameTextInputKind::ExternalSubtitleTitle => self.external_subtitle_title.as_ref(),
            FrameTextInputKind::SubtitleFontColorHex => self.subtitle_font_color.as_ref(),
            FrameTextInputKind::SubtitleOutlineColorHex => self.subtitle_outline_color.as_ref(),
        }
    }

    pub(in crate::app) const fn focus_mut(
        &mut self,
        kind: FrameTextInputKind,
    ) -> &mut Option<FocusHandle> {
        match kind {
            FrameTextInputKind::MaxConcurrency => &mut self.max_concurrency,
            FrameTextInputKind::OutputName => &mut self.output_name,
            FrameTextInputKind::AudioBitrate => &mut self.audio_bitrate,
            FrameTextInputKind::VideoCustomWidth => &mut self.video_width,
            FrameTextInputKind::VideoCustomHeight => &mut self.video_height,
            FrameTextInputKind::VideoBitrate => &mut self.video_bitrate,
            FrameTextInputKind::VideoMaxrate => &mut self.video_maxrate,
            FrameTextInputKind::VideoBufsize => &mut self.video_bufsize,
            FrameTextInputKind::VideoX264PsyRd => &mut self.video_x264_psy_rd,
            FrameTextInputKind::VideoX265PsyRd => &mut self.video_x265_psy_rd,
            FrameTextInputKind::VideoX265PsyRdoq => &mut self.video_x265_psy_rdoq,
            FrameTextInputKind::GifLoop => &mut self.gif_loop,
            FrameTextInputKind::PreviewStartTime => &mut self.preview_start_time,
            FrameTextInputKind::PreviewEndTime => &mut self.preview_end_time,
            FrameTextInputKind::MetadataTitle => &mut self.metadata_title,
            FrameTextInputKind::MetadataArtist => &mut self.metadata_artist,
            FrameTextInputKind::MetadataAlbum => &mut self.metadata_album,
            FrameTextInputKind::MetadataGenre => &mut self.metadata_genre,
            FrameTextInputKind::MetadataDate => &mut self.metadata_date,
            FrameTextInputKind::MetadataComment => &mut self.metadata_comment,
            FrameTextInputKind::MetadataServiceName => &mut self.metadata_service_name,
            FrameTextInputKind::MetadataServiceProvider => &mut self.metadata_service_provider,
            FrameTextInputKind::PresetName => &mut self.preset_name,
            FrameTextInputKind::ExternalSubtitleLanguage => &mut self.external_subtitle_language,
            FrameTextInputKind::ExternalSubtitleTitle => &mut self.external_subtitle_title,
            FrameTextInputKind::SubtitleFontColorHex => &mut self.subtitle_font_color,
            FrameTextInputKind::SubtitleOutlineColorHex => &mut self.subtitle_outline_color,
        }
    }

    pub(in crate::app) fn clear(&mut self, kind: FrameTextInputKind) {
        *self.focus_mut(kind) = None;
    }
}

pub(in crate::app) struct FrameTextInputUiState {
    pub(in crate::app) active: Option<FrameTextInputKind>,
    pub(in crate::app) runtimes: FrameTextInputStore,
    pub(in crate::app) focuses: FrameTextInputFocusStore,
    pub(in crate::app) cursor_visible: bool,
    pub(in crate::app) cursor_paused: bool,
    pub(in crate::app) cursor_epoch: usize,
    pub(in crate::app) cursor_task: Task<()>,
}

impl Default for FrameTextInputUiState {
    fn default() -> Self {
        Self {
            active: None,
            runtimes: FrameTextInputStore::default(),
            focuses: FrameTextInputFocusStore::default(),
            cursor_visible: false,
            cursor_paused: false,
            cursor_epoch: 0,
            cursor_task: Task::ready(()),
        }
    }
}
