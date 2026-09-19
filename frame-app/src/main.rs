// GUI 程序：声明 Windows 子系统，双击/命令行启动不再常驻控制台黑窗（上游缺失此声明）。
// 代价是 stdout/stderr 无处可去（eprintln 调试输出不可见）；产品日志走文件，不受影响。
// 开发需要看调试输出时，临时注掉本行重编译即可。
#![windows_subsystem = "windows"]

use frame_app::{
    app::{init_app, open_frame_window},
    app_info::FRAME_APP_NAME,
    assets::{self, FrameAssets},
};

fn main() {
    gpui_platform::application()
        .with_assets(FrameAssets)
        .run(|cx| {
            assets::load_frame_fonts(cx).expect("failed to load Frame fonts");
            open_frame_window(cx);
            init_app(cx, FRAME_APP_NAME);
        });
}
