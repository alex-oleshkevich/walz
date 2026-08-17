use mpris_server::{
    zbus, LocalPlayerInterface, LocalRootInterface, LocalServer, LoopStatus, Metadata,
    PlaybackStatus, Time, TrackId, Volume,
};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};

static MPRIS_RUNNING: AtomicBool = AtomicBool::new(false);

struct WalzPlayer {
    app: AppHandle,
}

impl LocalRootInterface for WalzPlayer {
    async fn raise(&self) -> zbus::fdo::Result<()> {
        if let Some(window) = self.app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
        Ok(())
    }

    async fn quit(&self) -> zbus::fdo::Result<()> {
        self.app.exit(0);
        Ok(())
    }

    async fn can_quit(&self) -> zbus::fdo::Result<bool> {
        Ok(true)
    }

    async fn fullscreen(&self) -> zbus::fdo::Result<bool> {
        Ok(false)
    }

    async fn set_fullscreen(&self, _fullscreen: bool) -> zbus::Result<()> {
        Ok(())
    }

    async fn can_set_fullscreen(&self) -> zbus::fdo::Result<bool> {
        Ok(false)
    }

    async fn can_raise(&self) -> zbus::fdo::Result<bool> {
        Ok(true)
    }

    async fn has_track_list(&self) -> zbus::fdo::Result<bool> {
        Ok(false)
    }

    async fn identity(&self) -> zbus::fdo::Result<String> {
        Ok("Walz".to_string())
    }

    async fn desktop_entry(&self) -> zbus::fdo::Result<String> {
        Ok("walz".to_string())
    }

    async fn supported_uri_schemes(&self) -> zbus::fdo::Result<Vec<String>> {
        Ok(vec![])
    }

    async fn supported_mime_types(&self) -> zbus::fdo::Result<Vec<String>> {
        Ok(vec!["audio/*".to_string()])
    }
}

impl LocalPlayerInterface for WalzPlayer {
    async fn next(&self) -> zbus::fdo::Result<()> {
        let _ = self.app.emit("mpris-next", ());
        Ok(())
    }

    async fn previous(&self) -> zbus::fdo::Result<()> {
        let _ = self.app.emit("mpris-previous", ());
        Ok(())
    }

    async fn pause(&self) -> zbus::fdo::Result<()> {
        let _ = self.app.emit("mpris-pause", ());
        Ok(())
    }

    async fn play_pause(&self) -> zbus::fdo::Result<()> {
        let _ = self.app.emit("mpris-play-pause", ());
        Ok(())
    }

    async fn stop(&self) -> zbus::fdo::Result<()> {
        let _ = self.app.emit("mpris-stop", ());
        Ok(())
    }

    async fn play(&self) -> zbus::fdo::Result<()> {
        let _ = self.app.emit("mpris-play", ());
        Ok(())
    }

    async fn seek(&self, offset: Time) -> zbus::fdo::Result<()> {
        let _ = self.app.emit("mpris-seek", offset.as_micros());
        Ok(())
    }

    async fn set_position(&self, _track_id: TrackId, position: Time) -> zbus::fdo::Result<()> {
        let _ = self.app.emit("mpris-set-position", position.as_micros());
        Ok(())
    }

    async fn open_uri(&self, _uri: String) -> zbus::fdo::Result<()> {
        Ok(())
    }

    async fn playback_status(&self) -> zbus::fdo::Result<PlaybackStatus> {
        Ok(PlaybackStatus::Stopped)
    }

    async fn loop_status(&self) -> zbus::fdo::Result<LoopStatus> {
        Ok(LoopStatus::None)
    }

    async fn set_loop_status(&self, _loop_status: LoopStatus) -> zbus::Result<()> {
        Ok(())
    }

    async fn rate(&self) -> zbus::fdo::Result<mpris_server::PlaybackRate> {
        Ok(1.0)
    }

    async fn set_rate(&self, _rate: mpris_server::PlaybackRate) -> zbus::Result<()> {
        Ok(())
    }

    async fn shuffle(&self) -> zbus::fdo::Result<bool> {
        Ok(false)
    }

    async fn set_shuffle(&self, _shuffle: bool) -> zbus::Result<()> {
        Ok(())
    }

    async fn metadata(&self) -> zbus::fdo::Result<Metadata> {
        Ok(Metadata::new())
    }

    async fn volume(&self) -> zbus::fdo::Result<Volume> {
        Ok(1.0)
    }

    async fn set_volume(&self, _volume: Volume) -> zbus::Result<()> {
        Ok(())
    }

    async fn position(&self) -> zbus::fdo::Result<Time> {
        Ok(Time::ZERO)
    }

    async fn minimum_rate(&self) -> zbus::fdo::Result<mpris_server::PlaybackRate> {
        Ok(1.0)
    }

    async fn maximum_rate(&self) -> zbus::fdo::Result<mpris_server::PlaybackRate> {
        Ok(1.0)
    }

    async fn can_go_next(&self) -> zbus::fdo::Result<bool> {
        Ok(true)
    }

    async fn can_go_previous(&self) -> zbus::fdo::Result<bool> {
        Ok(true)
    }

    async fn can_play(&self) -> zbus::fdo::Result<bool> {
        Ok(true)
    }

    async fn can_pause(&self) -> zbus::fdo::Result<bool> {
        Ok(true)
    }

    async fn can_seek(&self) -> zbus::fdo::Result<bool> {
        Ok(true)
    }

    async fn can_control(&self) -> zbus::fdo::Result<bool> {
        Ok(true)
    }
}

pub async fn start_server(app: AppHandle) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if MPRIS_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("MPRIS server already running".into());
    }

    let player = WalzPlayer { app };
    let server = LocalServer::new("walz", player).await?;
    server.run().await;
    Ok(())
}
