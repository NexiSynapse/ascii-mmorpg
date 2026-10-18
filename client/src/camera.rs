//! Client-only presentation camera controls.

/// Orbit camera state used by the GPU renderer. It is deliberately not part
/// of snapshots or the wire protocol: camera orientation is presentation-only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraState {
    /// Horizontal orbit angle, in radians.
    pub yaw: f32,
    /// Elevation above the ground plane, in radians.
    pub pitch: f32,
    /// Distance from the followed player focus.
    pub distance: f32,
}

impl CameraState {
    pub const MIN_PITCH: f32 = std::f32::consts::FRAC_PI_6; // 30 degrees
    pub const MAX_PITCH: f32 = 1.3613568; // 78 degrees

    pub fn orbit_cells(&mut self, dx: i16, dy: i16) {
        self.yaw -= f32::from(dx) * 0.012;
        self.pitch = (self.pitch + f32::from(dy) * 0.009).clamp(Self::MIN_PITCH, Self::MAX_PITCH);
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 58.0_f32.to_radians(),
            distance: 13.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CameraState;

    #[test]
    fn default_camera_is_elevated_for_top_down_play() {
        let camera = CameraState::default();
        assert!(camera.pitch > 45.0_f32.to_radians());
        assert!(camera.pitch < CameraState::MAX_PITCH);
    }

    #[test]
    fn horizontal_orbit_does_not_change_pitch() {
        let mut camera = CameraState::default();
        let pitch = camera.pitch;
        camera.orbit_cells(20, 0);
        assert_ne!(camera.yaw, 0.0);
        assert_eq!(camera.pitch, pitch);
    }

    #[test]
    fn vertical_orbit_is_clamped() {
        let mut camera = CameraState::default();
        camera.orbit_cells(0, i16::MAX);
        assert_eq!(camera.pitch, CameraState::MAX_PITCH);
        camera.orbit_cells(0, i16::MIN);
        assert_eq!(camera.pitch, CameraState::MIN_PITCH);
    }

    #[test]
    fn reset_restores_default() {
        let default_camera = CameraState::default();
        let mut camera = default_camera;
        camera.orbit_cells(30, -20);
        camera.reset();
        assert_eq!(camera, default_camera);
    }
}
