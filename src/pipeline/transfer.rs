//! Display transfer curves (OETF / EOTF) used between the scene-linear stage and the
//! display-referred creative stage of the pipeline.
//!
//! 16-bit decoder buffers are linear light. The pipeline applies white balance and
//! exposure to those linear values, then encodes them once with the buffer's transfer
//! curve before the tone and colour modules, which are tuned for display-encoded input.
//!
//! The curve parameters follow LibRaw's `gamma_curve(pwr, ts)`: `pwr` is the encoding
//! exponent (0.45 for BT.709) and `ts` the slope of the linear toe (4.5 for BT.709,
//! 0 for a pure power curve). Solving them the same way LibRaw does keeps a neutral
//! recipe bit-for-bit close to what LibRaw's own gamma output used to be.

/// A power curve with an optional linear toe, as LibRaw builds it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transfer {
    /// Encoding exponent (`gamm[0]`).
    power: f32,
    /// Slope of the linear toe (`gamm[1]`).
    slope: f32,
    /// End of the toe in encoded values (`gamm[2]`).
    toe_encoded: f32,
    /// End of the toe in linear values (`gamm[3]`).
    toe_linear: f32,
    /// Offset of the power segment (`gamm[4]`).
    offset: f32,
}

impl Transfer {
    /// Builds the curve for LibRaw's `gamm[0]` / `gamm[1]` pair.
    pub fn from_libraw(power: f64, slope: f64) -> Self {
        let mut g2 = 0.0f64;
        let mut g3 = 0.0f64;
        let mut g4 = 0.0f64;
        let mut bnd = [0.0f64, 0.0f64];
        bnd[usize::from(slope >= 1.0)] = 1.0;
        if slope != 0.0 && (slope - 1.0) * (power - 1.0) <= 0.0 {
            for _ in 0..48 {
                g2 = (bnd[0] + bnd[1]) / 2.0;
                if power != 0.0 {
                    let idx = usize::from(((g2 / slope).powf(-power) - 1.0) / power - 1.0 / g2 > -1.0);
                    bnd[idx] = g2;
                } else {
                    let idx = usize::from(g2 / (1.0 - 1.0 / g2).exp() < slope);
                    bnd[idx] = g2;
                }
            }
            g3 = g2 / slope;
            if power != 0.0 {
                g4 = g2 * (1.0 / power - 1.0);
            }
        }
        Self {
            power: power as f32,
            slope: slope as f32,
            toe_encoded: g2 as f32,
            toe_linear: g3 as f32,
            offset: g4 as f32,
        }
    }

    /// No curve: encoded values equal linear values.
    pub fn linear() -> Self {
        Self::from_libraw(1.0, 1.0)
    }

    /// ITU-R BT.709 (LibRaw's default output curve, also used for Rec.2020).
    pub fn bt709() -> Self {
        Self::from_libraw(0.45, 4.5)
    }

    /// sRGB / Display P3 piecewise curve.
    pub fn srgb() -> Self {
        Self::from_libraw(1.0 / 2.4, 12.92)
    }

    /// Linear light → display-encoded value. Values above 1.0 continue the power
    /// segment so highlight headroom survives until the tone stage clamps it.
    #[inline(always)]
    pub fn encode(&self, linear: f32) -> f32 {
        if !linear.is_finite() || linear <= 0.0 {
            0.0
        } else if linear < self.toe_linear {
            linear * self.slope
        } else {
            linear.powf(self.power) * (1.0 + self.offset) - self.offset
        }
    }

    /// Display-encoded value → linear light (inverse of [`Transfer::encode`]).
    #[inline(always)]
    pub fn decode(&self, encoded: f32) -> f32 {
        if !encoded.is_finite() || encoded <= 0.0 {
            0.0
        } else if encoded < self.toe_encoded {
            encoded / self.slope
        } else {
            ((encoded + self.offset) / (1.0 + self.offset)).powf(1.0 / self.power)
        }
    }
}

impl Default for Transfer {
    fn default() -> Self {
        Self::bt709()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bt709_matches_the_reference_curve() {
        let t = Transfer::bt709();
        // BT.709: 4.5 L below 0.018, 1.099 L^0.45 - 0.099 above
        assert!((t.encode(0.01) - 0.045).abs() < 1e-4);
        assert!((t.encode(0.5) - (1.099 * 0.5f32.powf(0.45) - 0.099)).abs() < 2e-3);
        assert!((t.encode(1.0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn srgb_matches_the_reference_curve() {
        let t = Transfer::srgb();
        assert!((t.encode(0.002) - 0.002 * 12.92).abs() < 1e-4);
        assert!((t.encode(0.5) - (1.055 * 0.5f32.powf(1.0 / 2.4) - 0.055)).abs() < 2e-3);
        assert!((t.encode(1.0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn pure_power_and_linear_curves() {
        let adobe = Transfer::from_libraw(1.0 / 2.199_218_75, 0.0);
        assert!((adobe.encode(0.25) - 0.25f32.powf(1.0 / 2.199_218_75)).abs() < 1e-5);
        let lin = Transfer::linear();
        for v in [0.0, 0.001, 0.3, 1.0, 2.0] {
            assert!((lin.encode(v) - v).abs() < 1e-5);
        }
    }

    #[test]
    fn decode_inverts_encode() {
        for t in [Transfer::bt709(), Transfer::srgb(), Transfer::from_libraw(1.0 / 1.8, 0.0)] {
            for i in 1..=200 {
                let v = i as f32 / 100.0;
                assert!((t.decode(t.encode(v)) - v).abs() < 1e-3 * v.max(0.01), "{v}");
            }
        }
    }
}
