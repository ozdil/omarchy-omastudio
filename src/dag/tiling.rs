//! High-Efficiency Tiling and Region-Of-Interest (ROI) Execution Engine
//! Splits massive 100 MP+ RAW images into cacheable 512x512 or 1024x1024 tiles.
//! Only active visible viewport tiles are decoded and evaluated, eliminating OOM crashes.

use std::sync::Arc;

pub const DEFAULT_TILE_SIZE: u32 = 512;

/// Geometric coordinate definition for a single processing tile
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileCoordinate {
    pub tile_x: u32,
    pub tile_y: u32,
    pub tile_size: u32,
    pub level_of_detail: u32, // Mipmap downsample level: 0 = full, 1 = 1/2, 2 = 1/4
}

impl TileCoordinate {
    pub fn pixel_bounds(&self, image_width: u32, image_height: u32) -> (u32, u32, u32, u32) {
        let step = self.tile_size << self.level_of_detail;
        let px = self.tile_x * step;
        let py = self.tile_y * step;
        let pw = (step).min(image_width.saturating_sub(px));
        let ph = (step).min(image_height.saturating_sub(py));
        (px, py, pw, ph)
    }
}

/// A cached evaluation chunk representing an executed tile
#[derive(Debug, Clone)]
pub struct TileBuffer {
    pub coord: TileCoordinate,
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub data: Arc<Vec<f32>>,
}

/// Spatial tile grid partitioner for high-resolution images
pub struct TileGrid {
    pub image_width: u32,
    pub image_height: u32,
    pub tile_size: u32,
    pub cols: u32,
    pub rows: u32,
}

impl TileGrid {
    pub fn new(image_width: u32, image_height: u32, tile_size: u32) -> Self {
        let ts = if tile_size == 0 { DEFAULT_TILE_SIZE } else { tile_size };
        let cols = (image_width + ts - 1) / ts;
        let rows = (image_height + ts - 1) / ts;
        Self {
            image_width,
            image_height,
            tile_size: ts,
            cols,
            rows,
        }
    }

    /// Computes which tiles intersect the current viewport view bounds
    pub fn visible_tiles(
        &self,
        view_x: u32,
        view_y: u32,
        view_w: u32,
        view_h: u32,
        lod: u32,
    ) -> Vec<TileCoordinate> {
        let step = self.tile_size << lod;
        let start_col = (view_x / step).min(self.cols.saturating_sub(1));
        let end_col = ((view_x + view_w + step - 1) / step).min(self.cols);

        let start_row = (view_y / step).min(self.rows.saturating_sub(1));
        let end_row = ((view_y + view_h + step - 1) / step).min(self.rows);

        let mut tiles = Vec::new();
        for r in start_row..end_row {
            for c in start_col..end_col {
                tiles.push(TileCoordinate {
                    tile_x: c,
                    tile_y: r,
                    tile_size: self.tile_size,
                    level_of_detail: lod,
                });
            }
        }
        tiles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_grid_partitioning() {
        let grid = TileGrid::new(3840, 2160, 512);
        assert_eq!(grid.cols, 8); // (3840 + 511) / 512 = 8
        assert_eq!(grid.rows, 5); // (2160 + 511) / 512 = 5

        // Visible tiles for full view
        let all_tiles = grid.visible_tiles(0, 0, 3840, 2160, 0);
        assert_eq!(all_tiles.len(), 40);

        // Visible tiles for localized 500x500 viewport at center
        let center_tiles = grid.visible_tiles(1000, 1000, 500, 500, 0);
        assert!(center_tiles.len() >= 2 && center_tiles.len() <= 4);
    }
}
