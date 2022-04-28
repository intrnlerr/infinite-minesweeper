use std::{
    io::{BufReader, BufWriter, Read, Write},
    mem::size_of,
};

use ggez::{
    conf::WindowSetup,
    event::{self, EventHandler, KeyCode, KeyMods, MouseButton},
    graphics::{self, spritebatch::SpriteIdx, Color, DrawParam, FilterMode, Rect},
    input, Context, ContextBuilder, GameResult,
};
use hashbrown::{HashMap, HashSet};

fn main() {
    let (mut ctx, event_loop) = ContextBuilder::new("infinite-minesweep", "in")
        .window_setup(WindowSetup {
            title: "infinite minesweeper".to_string(),
            samples: ggez::conf::NumSamples::One,
            vsync: true,
            icon: "".to_string(),
            srgb: true,
        })
        .build()
        .expect("fuck");

    let game = InfiniteMinesweeper::new(&mut ctx);
    println!("Hello, world!");
    event::run(ctx, event_loop, game);
}

#[derive(Debug, Clone, Copy)]
enum CellType {
    Unknown = 0,
    Revealed = 1,
    Mine = 2,
}

impl From<u32> for CellType {
    fn from(n: u32) -> Self {
        match n {
            0 => CellType::Unknown,
            1 => CellType::Revealed,
            2 => CellType::Mine,
            _ => panic!("bad!"),
        }
    }
}

/*
#[derive(Debug, Clone, Copy)]
struct BitSet32 {
    bits: u32,
}

impl BitSet32 {
    fn new() -> Self {
        Self { bits: 0 }
    }
    fn get(&self, pos: u32) -> bool {
        self.bits >> pos & 1 == 1
    }
    fn set(&mut self, pos: u32) {
        self.bits |= 1 << pos
    }
    fn unset(&mut self, pos: u32) {
        self.bits &= !(1 << pos)
    }
}*/

#[derive(Debug, Clone, Copy)]
struct Chunk {
    data: u32,  // 2 bits per "cell" means 16 cells or a 4x4
    flags: u16, // bitboard of where the user has flagged
}

impl Chunk {
    // 00 04 08 12
    // 01 05 09 13
    // 02 06 10 14
    // 03 07 11 15
    const SIZE: u32 = 4;
    const CHUNK_CELL: u32 = 0b11;
    const TOP_LEFT: u32 = 0;
    const BOT_LEFT: u32 = Self::SIZE - 1;
    const TOP_RIGHT: u32 = Self::SIZE * (Self::SIZE - 1);
    const BOT_RIGHT: u32 = Self::BOT_LEFT + Self::TOP_RIGHT;
    fn get_cell(&self, pos: u32) -> CellType {
        (self.data >> (pos * 2) & Self::CHUNK_CELL).into()
    }
    fn set_cell(&mut self, pos: u32, cell: CellType) {
        let n = cell as u32;
        self.data &= !(Self::CHUNK_CELL << pos * 2);
        self.data |= n << (pos * 2);
    }
    fn has_mine(&self, pos: u32) -> bool {
        match self.get_cell(pos) {
            CellType::Mine => true,
            _ => false,
        }
    }
    fn toggle_flag(&mut self, pos: u32) {
        self.flags ^= 1 << pos
    }
    fn get_flag(&self, pos: u32) -> bool {
        (self.flags >> pos) & 1 == 1
    }
    fn run_fn_south<F: FnMut(u32)>(&mut self, pos: u32, mut func: F) {
        if !Self::is_bottom(pos) {
            func(pos + 1);
        }
    }
    fn run_fn_north<F: FnMut(u32)>(&mut self, pos: u32, mut func: F) {
        if !Self::is_top(pos) {
            func(pos - 1);
        }
    }
    fn run_fn_east<F: FnMut(u32)>(&mut self, pos: u32, mut func: F) {
        if !Self::is_right(pos) {
            func(pos + Chunk::SIZE);
        }
    }
    fn run_fn_west<F: FnMut(u32)>(&mut self, pos: u32, mut func: F) {
        if !Self::is_left(pos) {
            func(pos - Chunk::SIZE);
        }
    }

    fn generate_chunk(rng: &mut impl rand::Rng) -> Chunk {
        let mut chunk = Chunk { data: 0, flags: 0 };
        // TODO: adjustable difficulty
        if rng.gen() {
            for _ in 0..4 {
                let pos = rng.gen_range(0..16);
                chunk.set_cell(pos, CellType::Mine);
            }
        }
        /*if !expand.is_empty() {
            println!("woah..");
        }*/
        chunk
    }
    fn is_top(n: u32) -> bool {
        n & 0x3 == 0
    }
    fn is_bottom(n: u32) -> bool {
        n & 0x3 == 0x3
    }
    fn is_left(n: u32) -> bool {
        n <= Chunk::BOT_LEFT
    }
    fn is_right(n: u32) -> bool {
        n >= Chunk::TOP_RIGHT
    }
    fn has_mine_neighbor(&self, pos: u32) -> bool {
        if pos > 3 {
            if self.has_mine(pos - Chunk::SIZE) {
                return true;
            }
            if !Self::is_top(pos) && self.has_mine(pos - 1 - Chunk::SIZE) {
                return true;
            }
            if !Self::is_bottom(pos) && self.has_mine(pos + 1 - Chunk::SIZE) {
                return true;
            }
        }
        if pos < Chunk::TOP_RIGHT {
            if self.has_mine(pos + Chunk::SIZE) {
                return true;
            }
            if !Self::is_top(pos) && self.has_mine(pos - 1 + Chunk::SIZE) {
                return true;
            }
            if !Self::is_bottom(pos) && self.has_mine(pos + 1 + Chunk::SIZE) {
                return true;
            }
        }
        if !Self::is_top(pos) {
            if self.has_mine(pos - 1) {
                return true;
            }
            if pos > Chunk::BOT_LEFT && self.has_mine(pos - 1 - Chunk::SIZE) {
                return true;
            }
            if pos < Chunk::TOP_RIGHT && self.has_mine(pos - 1 + Chunk::SIZE) {
                return true;
            }
        }
        if !Self::is_bottom(pos) {
            if self.has_mine(pos + 1) {
                return true;
            }
            if pos > Chunk::BOT_LEFT && self.has_mine(pos + 1 - Chunk::SIZE) {
                return true;
            }
            if pos < Chunk::TOP_RIGHT && self.has_mine(pos + 1 + Chunk::SIZE) {
                return true;
            }
        }
        false
    }
    fn expand_interior(&mut self, pos: u32) {
        if let CellType::Unknown = self.get_cell(pos) {
            self.set_cell(pos, CellType::Revealed);
            if self.has_mine_neighbor(pos) {
                return;
            }
            if pos > 3 {
                self.expand_interior(pos - Chunk::SIZE);
                if !Self::is_top(pos) {
                    self.expand_interior(pos - 1 - Chunk::SIZE);
                }
                if !Self::is_bottom(pos) {
                    self.expand_interior(pos + 1 - Chunk::SIZE);
                }
            }
            if pos < Chunk::TOP_RIGHT {
                self.expand_interior(pos + Chunk::SIZE);
                if !Self::is_top(pos) {
                    self.expand_interior(pos - 1 + Chunk::SIZE);
                }
                if !Self::is_bottom(pos) {
                    self.expand_interior(pos + 1 + Chunk::SIZE);
                }
            }
            if !Self::is_top(pos) {
                self.expand_interior(pos - 1);
                if pos > Chunk::BOT_LEFT {
                    self.expand_interior(pos - 1 - Chunk::SIZE);
                }
                if pos < Chunk::TOP_RIGHT {
                    self.expand_interior(pos - 1 + Chunk::SIZE);
                }
            }
            if !Self::is_bottom(pos) {
                self.expand_interior(pos + 1);
                if pos > Chunk::BOT_LEFT {
                    self.expand_interior(pos + 1 - Chunk::SIZE);
                }
                if pos < Chunk::TOP_RIGHT {
                    self.expand_interior(pos + 1 + Chunk::SIZE);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
struct ChunkPos {
    x: i32,
    y: i32,
}

impl ChunkPos {
    fn east(self) -> Self {
        ChunkPos {
            x: self.x + 1,
            y: self.y,
        }
    }
    fn west(self) -> Self {
        ChunkPos {
            x: self.x - 1,
            y: self.y,
        }
    }
    fn north(self) -> Self {
        ChunkPos {
            x: self.x,
            y: self.y - 1,
        }
    }
    fn south(self) -> Self {
        ChunkPos {
            x: self.x,
            y: self.y + 1,
        }
    }
}

struct InfiniteMinesweeper {
    // tile_image: graphics::Image,
    tile_batch: graphics::spritebatch::SpriteBatch,
    camera: ChunkPos,
    offset_x: i16,
    offset_y: i16,
    world: HashMap<ChunkPos, Chunk>,
    visible_chunks: HashMap<(i32, i32), [SpriteIdx; 16]>,
    first_click: bool,
    zoom: f32,
}

struct Neighbors {
    east: Chunk,
    west: Chunk,
    north: Chunk,
    south: Chunk,
    north_east: Chunk,
    north_west: Chunk,
    south_east: Chunk,
    south_west: Chunk,
}

struct CellNeighbors([CellPos; 8]);

impl CellNeighbors {
    fn new(
        east: CellPos,
        west: CellPos,
        north: CellPos,
        south: CellPos,
        northeast: (ChunkPos, u32),
        northwest: (ChunkPos, u32),
        southeast: (ChunkPos, u32),
        southwest: (ChunkPos, u32),
    ) -> CellNeighbors {
        CellNeighbors([
            east, west, north, south, northeast, northwest, southeast, southwest,
        ])
    }
}

type CellPos = (ChunkPos, u32);

trait Translate {
    fn north(self) -> Self;
    fn south(self) -> Self;
    fn east(self) -> Self;
    fn west(self) -> Self;
}

impl Translate for CellPos {
    fn north(self) -> Self {
        match self.1 {
            0 | 4 | 8 | 12 => (self.0.north(), self.1 + 3),
            _ => (self.0, self.1 - 1),
        }
    }

    fn south(self) -> Self {
        match self.1 {
            3 | 7 | 11 | 15 => (self.0.south(), self.1 - 3),
            _ => (self.0, self.1 + 1),
        }
    }

    fn east(self) -> Self {
        match self.1 {
            12..=15 => (self.0.east(), self.1 - 12),
            _ => (self.0, self.1 + Chunk::SIZE),
        }
    }

    fn west(self) -> Self {
        match self.1 {
            0..=3 => (self.0.west(), self.1 + 12),
            _ => (self.0, self.1 - Chunk::SIZE),
        }
    }
}

impl InfiniteMinesweeper {
    fn new(ctx: &mut Context) -> Self {
        let tile_image = graphics::Image::new(ctx, "/tiles.png").expect("no image :(");
        let mut ms = InfiniteMinesweeper {
            // tile_image: tile_image.clone(),
            tile_batch: graphics::spritebatch::SpriteBatch::new(tile_image),
            camera: ChunkPos { x: 0, y: 0 },
            offset_x: 0,
            offset_y: 0,
            world: HashMap::new(),
            visible_chunks: HashMap::new(),
            first_click: true,
            zoom: 2.0,
        };
        ms.load_save();
        graphics::set_resizable(ctx, true).unwrap();
        ms.tile_batch.set_filter(FilterMode::Nearest);
        ms.update_tile_batch(ctx).unwrap();
        ms.update_tile_batch(ctx).unwrap();
        ms
    }

    fn re_explore_chunk(&mut self, chunk: ChunkPos, pos: u32) {
        if let Some(re_explore) = self.world.get_mut(&chunk) {
            if let CellType::Revealed = re_explore.get_cell(pos) {
                return;
            }
            let mut to_re_explore = Vec::new();
            re_explore.expand_interior(pos);
            for cell_pos in 0..4 {
                if let CellType::Revealed = re_explore.get_cell(cell_pos) {
                    // re-explore east
                    to_re_explore.push((chunk.east(), cell_pos + 12));
                }
            }
            for (chunk, pos) in to_re_explore {
                if self.get_neighboring_mines_from_pos((chunk, pos)) != 0 {
                    continue;
                }
                self.re_explore_chunk(chunk, pos)
            }
        }
    }

    /*fn generate_chunk(&mut self, pos: ChunkPos) -> Chunk {

        let mut expand = BitSet32::new();
        if let Some(chunk) = self.world.get(&pos.east()) {
            for pos in Chunk::TOP_LEFT..=Chunk::BOT_LEFT {
                if let CellType::Revealed = chunk.get_cell(pos) {
                    expand.set(pos + (Chunk::SIZE * 3));
                }
            }
        }
        if let Some(chunk) = self.world.get(&pos.west()) {
            for pos in Chunk::TOP_RIGHT..=Chunk::BOT_RIGHT {
                if let CellType::Revealed = chunk.get_cell(pos) {
                    expand.set(pos - (Chunk::SIZE * 3));
                }
            }
        }
        if let Some(chunk) = self.world.get(&pos.north()) {
            for pos in [3, 7, 11, 15] {
                if let CellType::Revealed = chunk.get_cell(pos) {
                    expand.set(pos - 3);
                }
            }
        }
        if let Some(chunk) = self.world.get(&pos.south()) {
            for pos in [0, 4, 8, 12] {
                if let CellType::Revealed = chunk.get_cell(pos) {
                    expand.set(pos + 3);
                }
            }
        }
        let chunk = Chunk::generate_chunk(&mut rand::thread_rng(), expand);
        for cell_pos in 0..4 {
            if let CellType::Revealed = chunk.get_cell(cell_pos) {
                if self.has_known_mine_neighbor((pos, cell_pos)) {
                    continue;
                }
                // re-explore north
                self.re_explore_chunk(pos.west(), cell_pos + 12)
            }
        }
        chunk
    } */

    // modifying the returned chunk WILL NOT update it in the world
    fn get_chunk(&mut self, pos: ChunkPos) -> Chunk {
        *self
            .world
            .entry(pos)
            .or_insert_with(|| Chunk::generate_chunk(&mut rand::thread_rng()))
    }

    fn get_chunk_no_generate(&mut self, pos: ChunkPos) -> Option<Chunk> {
        self.world.get(&pos).cloned()
    }

    fn get_cell_neighbors(pos: CellPos) -> CellNeighbors {
        CellNeighbors::new(
            pos.east(),
            pos.west(),
            pos.north(),
            pos.south(),
            pos.north().east(),
            pos.north().west(),
            pos.south().east(),
            pos.south().west(),
        )
        /* match pos {
            Chunk::TOP_LEFT => {
                // top left
                CellNeighbors{
                    north_west: (chunk.north().west(), Chunk::BOT_RIGHT),
                    north_east: (chunk.north(), Chunk::BOT_LEFT + Chunk::SIZE),
                    south_west: (chunk.west(), Chunk::TOP_RIGHT + 1),
                    south_east: (chunk, pos + 1 + Chunk::SIZE),
                    north: (chunk.north(), Chunk::BOT_LEFT),
                    south: (chunk, pos + 1),
                    east: (chunk, pos + Chunk::SIZE),
                    west: (chunk.west(), Chunk::TOP_RIGHT),
                }
            }
            Chunk::BOT_LEFT => {
                CellNeighbors{
                    north_west: (chunk.west(), Chunk::BOT_RIGHT - 1),
                    north_east: (chunk, pos + Chunk::SIZE - 1),
                    south_west: (chunk.south().west(), Chunk::TOP_RIGHT),
                    south_east: (chunk.south(), Chunk::TOP_LEFT + Chunk::SIZE),
                    north: (chunk, pos - 1),
                    south: (chunk.south(), Chunk::TOP_LEFT),
                    east: (chunk, pos + Chunk::SIZE),
                    west: (chunk.west(), Chunk::BOT_RIGHT),
                }
            }
            Chunk::TOP_RIGHT => {
                CellNeighbors{
                    north_west: (chunk.north(), Chunk::BOT_RIGHT - Chunk::SIZE),
                    north_east: (chunk.north().east(), Chunk::BOT_LEFT),
                    south_west: (chunk, Chunk::TOP_RIGHT - Chunk::SIZE + 1),
                    south_east: (chunk.east(), Chunk::TOP_LEFT + 1),
                    north: (chunk.north(), Chunk::BOT_RIGHT),
                    south: (chunk, Chunk::TOP_RIGHT + 1),
                    east: (chunk.east(), Chunk::TOP_LEFT),
                    west: (chunk, Chunk::TOP_RIGHT - Chunk::SIZE),
                }
            }
            Chunk::BOT_RIGHT => {
                CellNeighbors{
                    north_west: (chunk, pos + Chunk::SIZE - 1),
                    north_east: (chunk.east(), Chunk::BOT_RIGHT),
                    south_west: (chunk.south().west(), Chunk::TOP_RIGHT),
                    south_east: (chunk.south(), Chunk::TOP_RIGHT + Chunk::SIZE),
                    north: (chunk, pos - 1),
                    south: (chunk.south(), Chunk::TOP_LEFT),
                    east: (chunk, pos + Chunk::SIZE),
                    west: (chunk.west(), Chunk::BOT_RIGHT),
                }
            }
            _ => CellNeighbors{
                north_west: (chunk, pos + Chunk::SIZE - 1),
                north_east: (chunk.east(), Chunk::BOT_RIGHT),
                south_west: (chunk.south().west(), Chunk::TOP_RIGHT),
                south_east: (chunk.south(), Chunk::TOP_RIGHT + Chunk::SIZE),
                north: (chunk, pos - 1),
                south: (chunk.south(), Chunk::TOP_LEFT),
                east: (chunk, pos + Chunk::SIZE),
                west: (chunk.west(), Chunk::BOT_RIGHT),
            }
        }*/
    }

    fn get_neighbors(&mut self, pos: ChunkPos) -> Neighbors {
        Neighbors {
            north_west: self.get_chunk(ChunkPos {
                x: pos.x - 1,
                y: pos.y - 1,
            }), // top left
            north: self.get_chunk(ChunkPos {
                x: pos.x,
                y: pos.y - 1,
            }), // top
            north_east: self.get_chunk(ChunkPos {
                x: pos.x + 1,
                y: pos.y - 1,
            }), // top right
            west: self.get_chunk(ChunkPos {
                x: pos.x - 1,
                y: pos.y,
            }), // left
            east: self.get_chunk(ChunkPos {
                x: pos.x + 1,
                y: pos.y,
            }), // right
            south_west: self.get_chunk(ChunkPos {
                x: pos.x - 1,
                y: pos.y + 1,
            }), // bot left
            south: self.get_chunk(ChunkPos {
                x: pos.x,
                y: pos.y + 1,
            }), // bot
            south_east: self.get_chunk(ChunkPos {
                x: pos.x + 1,
                y: pos.y + 1,
            }), // bot right
        }
    }

    fn get_neighbors_no_generate(&mut self, pos: ChunkPos) -> Option<Neighbors> {
        Some(Neighbors {
            north_west: *self.world.get(&pos.north().west())?, // top left
            north: *self.world.get(&pos.north())?,             // top
            north_east: *self.world.get(&pos.north().east())?, // top right
            west: *self.world.get(&pos.west())?,               // left
            east: *self.world.get(&pos.east())?,               // right
            south_west: *self.world.get(&pos.south().west())?, // bot left
            south: *self.world.get(&pos.south())?,             // bot
            south_east: *self.world.get(&pos.south().east())?, // bot right
        })
    }

    fn get_neighboring_mines(
        &self,
        chunk: &Chunk,
        neighbors: &Neighbors,
        col: u32,
        row: u32,
    ) -> u8 {
        let mut neighboring_mines = 0;
        let pos = col * Chunk::SIZE + row;
        if col > 0 {
            if chunk.has_mine(pos - Chunk::SIZE) {
                neighboring_mines += 1;
            }
        } else {
            // need to access east side of west neighbor
            if neighbors.west.has_mine(Chunk::TOP_RIGHT + row) {
                neighboring_mines += 1;
            }
        }
        if col < Chunk::SIZE - 1 {
            if chunk.has_mine(pos + Chunk::SIZE) {
                neighboring_mines += 1;
            }
        } else {
            // need to access west most side of east neighbor
            if neighbors.east.has_mine(Chunk::TOP_LEFT + row) {
                neighboring_mines += 1;
            }
        }
        if row > 0 {
            if chunk.has_mine(pos - 1) {
                neighboring_mines += 1;
            }
        } else {
            if neighbors
                .north
                .has_mine(Chunk::BOT_LEFT + col * Chunk::SIZE)
            {
                neighboring_mines += 1;
            }
        }
        if row < Chunk::SIZE - 1 {
            if chunk.has_mine(pos + 1) {
                neighboring_mines += 1;
            }
        } else {
            if neighbors
                .south
                .has_mine(Chunk::TOP_LEFT + col * Chunk::SIZE)
            {
                neighboring_mines += 1;
            }
        }
        // top left
        if col > 0 && row > 0 {
            if let CellType::Mine = chunk.get_cell(pos - 1 - Chunk::SIZE) {
                neighboring_mines += 1;
            }
        } else if col == 0 && row == 0 {
            if neighbors.north_west.has_mine(Chunk::BOT_RIGHT) {
                neighboring_mines += 1;
            }
        } else if col == 0 {
            if neighbors.west.has_mine(Chunk::TOP_RIGHT + row - 1) {
                neighboring_mines += 1;
            }
        } else if row == 0 {
            if neighbors
                .north
                .has_mine(Chunk::BOT_LEFT + (col - 1) * Chunk::SIZE)
            {
                neighboring_mines += 1;
            }
        }
        // top right
        if col < Chunk::SIZE - 1 && row > 0 {
            if let CellType::Mine = chunk.get_cell(pos - 1 + Chunk::SIZE) {
                neighboring_mines += 1;
            }
        } else if col == Chunk::SIZE - 1 && row == 0 {
            if neighbors.north_east.has_mine(Chunk::BOT_LEFT) {
                neighboring_mines += 1;
            }
        } else if col == Chunk::SIZE - 1 {
            if neighbors.east.has_mine(Chunk::TOP_LEFT + row - 1) {
                neighboring_mines += 1;
            }
        } else if row == 0 {
            if neighbors
                .north
                .has_mine(Chunk::BOT_LEFT + (col + 1) * Chunk::SIZE)
            {
                neighboring_mines += 1;
            }
        }
        // bottom left
        if col > 0 && row < Chunk::SIZE - 1 {
            if let CellType::Mine = chunk.get_cell(pos + 1 - Chunk::SIZE) {
                neighboring_mines += 1;
            }
        } else if col == 0 && row == Chunk::SIZE - 1 {
            if neighbors.south_west.has_mine(Chunk::TOP_RIGHT) {
                neighboring_mines += 1;
            }
        } else if col == 0 {
            if neighbors.west.has_mine(Chunk::TOP_RIGHT + row + 1) {
                neighboring_mines += 1;
            }
        } else if row == Chunk::SIZE - 1 {
            if neighbors
                .south
                .has_mine(Chunk::TOP_LEFT + (col - 1) * Chunk::SIZE)
            {
                neighboring_mines += 1;
            }
        }
        // bottom right
        if col < Chunk::SIZE - 1 && row < Chunk::SIZE - 1 {
            if let CellType::Mine = chunk.get_cell(pos + 1 + Chunk::SIZE) {
                neighboring_mines += 1;
            }
        } else if col == Chunk::SIZE - 1 && row == Chunk::SIZE - 1 {
            if neighbors.south_east.has_mine(Chunk::TOP_LEFT) {
                neighboring_mines += 1;
            }
        } else if col == Chunk::SIZE - 1 {
            if neighbors.east.has_mine(Chunk::TOP_LEFT + row + 1) {
                neighboring_mines += 1;
            }
        } else if row == Chunk::SIZE - 1 {
            if neighbors
                .south
                .has_mine(Chunk::TOP_LEFT + (col + 1) * Chunk::SIZE)
            {
                neighboring_mines += 1;
            }
        }
        neighboring_mines
    }

    fn get_neighboring_mines_from_pos(&mut self, (chunk_pos, pos): (ChunkPos, u32)) -> u8 {
        let chunk = self.get_chunk(chunk_pos);
        let neighbors = self.get_neighbors(chunk_pos);
        self.get_neighboring_mines(&chunk, &neighbors, pos / Chunk::SIZE, pos % Chunk::SIZE)
    }

    fn get_tile_src(&mut self, chunk: &Chunk, neighbors: &Neighbors, n: u32) -> Rect {
        let cell_type = chunk.get_cell(n);
        if chunk.get_flag(n) {
            return graphics::Rect {
                x: 0.5,
                y: 0.0,
                w: 0.25,
                h: 0.25,
            };
        }
        match cell_type {
            CellType::Revealed => {
                let neighboring_mines = self.get_neighboring_mines(
                    &chunk,
                    &neighbors,
                    n / Chunk::SIZE,
                    n % Chunk::SIZE,
                );
                if neighboring_mines == 0 {
                    graphics::Rect {
                        x: 0.25,
                        y: 0.0,
                        w: 0.25,
                        h: 0.25,
                    }
                } else {
                    graphics::Rect {
                        x: ((neighboring_mines - 1) % 4) as f32 * 0.25,
                        y: 0.25 + (0.25 * ((neighboring_mines - 1) / 4) as f32),
                        w: 0.25,
                        h: 0.25,
                    }
                }
            }
            /*CellType::Mine => graphics::Rect {
                x: 0.75,
                y: 0.0,
                w: 0.25,
                h: 0.25,
            },*/
            _ => graphics::Rect {
                x: 0.0,
                y: 0.0,
                w: 0.25,
                h: 0.25,
            },
        }
    }

    // returns (horizontal_extents, vertical_extents)
    fn get_chunk_extents(&self, ctx: &Context) -> (i32, i32) {
        let (size_x, size_y) = graphics::size(ctx);
        (
            (size_x / (CHUNK_PX_SIZEF * self.zoom)) as i32 + 1,
            (size_y / (CHUNK_PX_SIZEF * self.zoom)) as i32,
        )
    }

    fn update_tile_batch(&mut self, ctx: &mut Context) -> GameResult<()> {
        let (horizontal_extents, vertical_extents) = self.get_chunk_extents(ctx);
        for x in -1..=horizontal_extents {
            for y in -1..=vertical_extents {
                if let Some(&indexes) = self.visible_chunks.get(&(x, y)) {
                    let chunk_pos = ChunkPos {
                        x: x - self.camera.x,
                        y: y + self.camera.y,
                    };
                    let chunk = self.get_chunk(chunk_pos);
                    let neighbors = self.get_neighbors(chunk_pos);
                    for (index, n) in indexes.into_iter().zip(0..16_u32) {
                        let src = self.get_tile_src(&chunk, &neighbors, n);
                        self.tile_batch.set(
                            index,
                            DrawParam::new().src(src).dest(graphics::mint::Point2 {
                                x: x as f32 * CHUNK_PX_SIZEF
                                    + ((n / Chunk::SIZE) * (TILE_PX_SIZE32)) as f32,
                                y: y as f32 * CHUNK_PX_SIZEF
                                    + ((n % Chunk::SIZE) * (TILE_PX_SIZE32)) as f32,
                            }),
                        )?;
                    }
                } else {
                    // generate a new set of indicies
                    let mut sprites = Vec::with_capacity(16);
                    let chunk_pos = ChunkPos {
                        x: x - self.camera.x,
                        y: y + self.camera.y,
                    };
                    let chunk = self.get_chunk(chunk_pos);
                    let neighbors = self.get_neighbors(chunk_pos);
                    for n in 0..16 {
                        let src = self.get_tile_src(&chunk, &neighbors, n);
                        sprites.push(self.tile_batch.add(DrawParam::new().src(src).dest(
                            graphics::mint::Point2 {
                                x: x as f32 * CHUNK_PX_SIZEF
                                    + ((n / Chunk::SIZE) * (TILE_PX_SIZE32)) as f32,
                                y: y as f32 * CHUNK_PX_SIZEF
                                    + ((n % Chunk::SIZE) * (TILE_PX_SIZE32)) as f32,
                            },
                        )))
                    }
                    self.visible_chunks
                        .insert((x, y), sprites.try_into().unwrap());
                }
            }
        }
        Ok(())
    }

    fn prune_tile_batch(&mut self) {
        self.tile_batch.clear();
        self.visible_chunks.clear();
    }

    /*
    fn draw_chunk(&mut self, ctx: &mut Context, pos: ChunkPos, x: i32, y: i32) -> GameResult<()> {
        let chunk = *self.get_chunk(pos);
        let neighbors = self.get_neighbors(pos);
        for col in 0..Chunk::SIZE {
            for row in 0..Chunk::SIZE {
                let cell_type = chunk.get_cell(col * Chunk::SIZE + row);
                let src = match cell_type {
                    CellType::Revealed => {
                        let neighboring_mines =
                            self.get_neighboring_mines(&chunk, &neighbors, col, row);
                        if neighboring_mines == 0 {
                            graphics::Rect {
                                x: 0.25,
                                y: 0.0,
                                w: 0.25,
                                h: 0.25,
                            }
                        } else {
                            graphics::Rect {
                                x: ((neighboring_mines - 1) % 4) as f32 * 0.25,
                                y: 0.25 + (0.25 * ((neighboring_mines - 1) / 4) as f32),
                                w: 0.25,
                                h: 0.25,
                            }
                        }
                    }
                    /*CellType::Mine => graphics::Rect {
                        x: 0.75,
                        y: 0.0,
                        w: 0.25,
                        h: 0.25,
                    },*/
                    _ => graphics::Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 0.25,
                        h: 0.25,
                    },
                };
                graphics::draw(
                    ctx,
                    &self.tile_image,
                    DrawParam::new()
                        .dest(ggez::mint::Point2 {
                            x: (col * 8) as f32 + (x as f32),
                            y: (row * 8) as f32 + (y as f32),
                        })
                        .src(src),
                )?;
            }
        }
        Ok(())
    } */

    fn get_mouse_position(&self, ctx: &mut Context) -> (ChunkPos, u32) {
        let chunk_px_size = (CHUNK_PX_SIZEF * self.zoom) as i32;
        let tile_px_size = (TILE_PX_SIZEF * self.zoom) as i32;
        let pos = input::mouse::position(ctx);
        let aligned_x = pos.x as i32 - self.offset_x as i32;
        let chunk_x = aligned_x / chunk_px_size - self.camera.x;
        let interior_x = aligned_x / tile_px_size % (Chunk::SIZE as i32);

        let aligned_y = pos.y as i32 - self.offset_y as i32;
        let chunk_y = aligned_y / chunk_px_size + self.camera.y;
        let interior_y = aligned_y / tile_px_size % (Chunk::SIZE as i32);
        (
            ChunkPos {
                x: chunk_x,
                y: chunk_y,
            },
            interior_x as u32 * Chunk::SIZE + (interior_y as u32),
        )
    }

    fn get_cell_no_generate(&self, (chunk, pos): CellPos) -> Option<CellType> {
        self.world.get(&chunk).map(|chunk| chunk.get_cell(pos))
    }

    fn reveal_empty_connected(&mut self, ctx: &Context, seed: (ChunkPos, u32)) {
        let (size_x, size_y) = graphics::size(ctx);
        let mut stack = vec![seed];
        let mut visited = HashSet::new();
        // be careful not to loop infinitely
        // only explore tiles within the viewing space!
        let viewable_min_x = -1 - self.camera.x;
        let viewable_max_x = (size_x / (CHUNK_PX_SIZEF * self.zoom)) as i32 - self.camera.x + 4;
        let viewable_min_y = self.camera.y - 4;
        let viewable_max_y = (size_y / (CHUNK_PX_SIZEF * self.zoom)) as i32 + self.camera.y + 4;
        println!(
            "{}..{} {}..{}",
            viewable_min_x, viewable_max_x, viewable_min_y, viewable_max_y
        );
        let viewable_x = viewable_min_x..=viewable_max_x;
        let viewable_y = viewable_min_y..viewable_max_y;
        println!("{:?}", stack);
        while let Some((chunk_pos, pos)) = stack.pop() {
            if !viewable_x.contains(&chunk_pos.x) || !viewable_y.contains(&chunk_pos.y) {
                continue;
            }
            if visited.contains(&(chunk_pos, pos)) {
                continue;
            }
            visited.insert((chunk_pos, pos));
            if let Some(chunk) = self.world.get_mut(&chunk_pos) {
                chunk.set_cell(pos, CellType::Revealed);
                let chunk = *chunk;
                let neighbors = Self::get_cell_neighbors((chunk_pos, pos));
                let mut mine_count = 0;
                for neighbor_pos in neighbors.0 {
                    if let Some(CellType::Mine) = self.get_cell_no_generate(neighbor_pos) {
                        mine_count += 1;
                    }
                }
                if mine_count > 0 {
                    continue;
                }
                stack.extend(neighbors.0);
            } else {
                // println!("???");
            }
        }
    }

    fn get_flag(&mut self, (chunk, pos): CellPos) -> bool {
        self.get_chunk(chunk).get_flag(pos)
    }

    fn get_cell(&mut self, (chunk, pos): CellPos) -> CellType {
        self.get_chunk(chunk).get_cell(pos)
    }

    fn set_cell(&mut self, (chunk, pos): (ChunkPos, u32), cell_type: CellType) {
        self.world.get_mut(&chunk).unwrap().set_cell(pos, cell_type);
    }
    fn has_mine_neighbor(&mut self, pos: CellPos) -> bool {
        Self::get_cell_neighbors(pos)
            .0
            .into_iter()
            .any(|pos| match self.get_cell(pos) {
                CellType::Mine => true,
                _ => false,
            })
    }
    fn has_known_mine_neighbor(&self, (chunk, pos): CellPos) -> bool {
        let neighbors = Self::get_cell_neighbors((chunk, pos));
        neighbors.0.iter().any(|(chunk, pos)| {
            self.world
                .get(chunk)
                .map_or(false, |chunk| chunk.has_mine(*pos))
        })
    }
    fn load_save(&mut self) {
        if let Ok(file) = std::fs::File::open("save") {
            let mut file = BufReader::new(file);
            let mut version = [0; 1];
            file.read_exact(&mut version).unwrap();
            let mut i32_buf = [0; 4];
            file.read_exact(&mut i32_buf).unwrap();
            self.camera.x = i32::from_le_bytes(i32_buf);
            file.read_exact(&mut i32_buf).unwrap();
            self.camera.y = i32::from_le_bytes(i32_buf);
            let mut chunk_data = [0; size_of::<i32>() * 2 + size_of::<u32>() + size_of::<u16>()];
            while let Ok(_) = file.read_exact(&mut chunk_data) {
                let x = i32::from_le_bytes(chunk_data[0..4].try_into().unwrap());
                let y = i32::from_le_bytes(chunk_data[4..8].try_into().unwrap());
                let data = u32::from_le_bytes(chunk_data[8..12].try_into().unwrap());
                let flags = u16::from_le_bytes(chunk_data[12..14].try_into().unwrap());
                self.world.insert(ChunkPos { x, y }, Chunk { data, flags });
            }
        } else {
            println!("no save file");
        }
    }
    fn write_save(&mut self) {
        if let Ok(file) = std::fs::File::create("save") {
            let mut file = BufWriter::new(file);
            file.write_all(&[0]).unwrap();
            file.write_all(&self.camera.x.to_le_bytes()).unwrap();
            file.write_all(&self.camera.y.to_le_bytes()).unwrap();
            for (pos, chunk) in &self.world {
                file.write_all(&pos.x.to_le_bytes()).unwrap();
                file.write_all(&pos.y.to_le_bytes()).unwrap();
                file.write_all(&chunk.data.to_le_bytes()).unwrap();
                file.write_all(&chunk.flags.to_le_bytes()).unwrap();
            }
        } else {
            println!("save file open :(");
        }
    }

    fn explore_new_chunks(&mut self, ctx: &Context, camera_delta_x: i32, camera_delta_y: i32) {
        let old_camera_x = self.camera.x - camera_delta_x;
        let old_camera_y = self.camera.y + camera_delta_y;
        let (horizontal_extents, vertical_extents) = self.get_chunk_extents(ctx);
        let check_top = camera_delta_y > 0;
        let mut positions_to_visit = Vec::new();
        // visit the edges of the old area
        for chunk_y in if check_top {
            old_camera_y - 2..old_camera_y
        } else {
            old_camera_y + vertical_extents..old_camera_y + vertical_extents + 1
        } {
            for chunk_x in -2 - old_camera_x..horizontal_extents - old_camera_x {
                for pos in if check_top {
                    [0, 4, 8, 12]
                } else {
                    [3, 7, 11, 15]
                } {
                    let chunk_pos = ChunkPos {
                        x: chunk_x,
                        y: chunk_y,
                    };
                    if let Some(chunk) = self.get_chunk_no_generate(chunk_pos) {
                        if let CellType::Revealed = chunk.get_cell(pos) {
                            if self.has_mine_neighbor((chunk_pos, pos)) {
                                continue;
                            }
                            let target_pos = if check_top {
                                chunk_pos.north()
                            } else {
                                chunk_pos.south()
                            };
                            let target_cell =
                                (target_pos, if check_top { pos + 3 } else { pos - 3 });
                            positions_to_visit.push(target_cell);
                            positions_to_visit.push(target_cell.east());
                            positions_to_visit.push(target_cell.west());
                        }
                        // self.world.get_mut(&chunk_pos).unwrap().data = 0xaaaaaaaa;
                        // self.world.get_mut(&chunk_pos).unwrap().data = 0xaaaaaaaa;
                    }
                }
            }
        }
        let check_left = camera_delta_x > 0;
        for chunk_x in if check_left {
            -2 - old_camera_x..-old_camera_x
        } else {
            horizontal_extents - old_camera_x - 1..horizontal_extents - old_camera_x + 2
        } {
            for chunk_y in old_camera_y - 2..=old_camera_y + vertical_extents {
                for pos in if check_left {
                    Chunk::TOP_LEFT..Chunk::BOT_LEFT
                } else {
                    Chunk::TOP_RIGHT..Chunk::BOT_RIGHT
                } {
                    let chunk_pos = ChunkPos {
                        x: chunk_x,
                        y: chunk_y,
                    };
                    if let Some(chunk) = self.get_chunk_no_generate(chunk_pos) {
                        if let CellType::Revealed = chunk.get_cell(pos) {
                            if self.has_mine_neighbor((chunk_pos, pos)) {
                                continue;
                            }
                            let target_pos = if check_left {
                                chunk_pos.west()
                            } else {
                                chunk_pos.east()
                            };
                            let target_cell =
                                (target_pos, if check_left { pos + 12 } else { pos - 12 });
                            positions_to_visit.push(target_cell);
                            positions_to_visit.push(target_cell.north());
                            positions_to_visit.push(target_cell.south());
                        }
                        // self.world.get_mut(&chunk_pos).unwrap().data = 0x55555555;
                    }
                }
            }
        }
        // println!("{}", positions_to_visit.len());
        let viewable_range_x = -4 - self.camera.x..horizontal_extents - self.camera.x + 2;
        let viewable_range_y = self.camera.y - 4..=self.camera.y + vertical_extents + 2;
        while let Some((chunk_pos, pos)) = positions_to_visit.pop() {
            if !viewable_range_x.contains(&chunk_pos.x) || !viewable_range_y.contains(&chunk_pos.y)
            {
                continue;
            }
            match self.get_cell((chunk_pos, pos)) {
                CellType::Revealed | CellType::Mine => continue,
                CellType::Unknown => {
                    self.set_cell((chunk_pos, pos), CellType::Revealed);
                    if self.get_neighboring_mines_from_pos((chunk_pos, pos)) == 0 {
                        positions_to_visit.extend(Self::get_cell_neighbors((chunk_pos, pos)).0);
                    }
                }
            };
        }
    }
}

const TILE_PX_SIZE8: i8 = 8;
const TILE_PX_SIZE32: u32 = TILE_PX_SIZE8 as u32;
const CHUNK_PX_SIZE8: i8 = TILE_PX_SIZE8 * 4;
// const CHUNK_PX_SIZE16: i16 = CHUNK_PX_SIZE8 as i16;
const TILE_PX_SIZEF: f32 = TILE_PX_SIZE8 as f32;
const CHUNK_PX_SIZEF: f32 = CHUNK_PX_SIZE8 as f32;

impl EventHandler for InfiniteMinesweeper {
    fn key_up_event(&mut self, ctx: &mut Context, keycode: KeyCode, keymods: KeyMods) {
        match keycode {
            KeyCode::Equals => {
                self.zoom *= 2.0;
                self.prune_tile_batch();
                self.update_tile_batch(ctx).unwrap();
                if self.zoom > 0.5 {
                    self.tile_batch.set_filter(FilterMode::Nearest);
                }
                self.offset_x = 0;
                self.offset_y = 0;
            }
            KeyCode::Minus => {
                self.zoom = (self.zoom / 2.0).max(0.25);
                if self.zoom < 1.0 {
                    self.tile_batch.set_filter(FilterMode::Linear);
                }
                self.prune_tile_batch();
                self.update_tile_batch(ctx).unwrap();
                self.offset_x = 0;
                self.offset_y = 0;
            }
            _ => (),
        }
    }

    fn resize_event(&mut self, ctx: &mut Context, width: f32, height: f32) {
        graphics::set_screen_coordinates(
            ctx,
            Rect {
                x: 0.0,
                y: 0.0,
                w: width,
                h: height,
            },
        )
        .unwrap();
        self.update_tile_batch(ctx).unwrap();
    }

    fn mouse_button_up_event(&mut self, ctx: &mut Context, button: MouseButton, x: f32, y: f32) {
        let mut should_update = false;
        match button {
            MouseButton::Right => {
                let (chunk_pos, pos) = self.get_mouse_position(ctx);
                if let Some(chunk) = self.world.get_mut(&chunk_pos) {
                    match chunk.get_cell(pos) {
                        CellType::Revealed => (),
                        _ => {
                            chunk.toggle_flag(pos);
                            should_update = true;
                        }
                    }
                }
            }
            MouseButton::Left => {
                if !input::keyboard::is_key_pressed(ctx, KeyCode::LControl) {
                    let (chunk_pos, pos) = self.get_mouse_position(ctx);
                    if self.first_click {
                        self.first_click = false;
                        if let Some(chunk) = self.world.get_mut(&chunk_pos) {
                            chunk.data = 0;
                        }
                    }
                    if let Some(chunk) = self.world.get_mut(&chunk_pos) {
                        match chunk.get_cell(pos) {
                            CellType::Mine => {
                                if !chunk.get_flag(pos) {
                                    // TODO: implement losing more
                                    println!("youve lose!!!");
                                }
                            }
                            CellType::Unknown => {
                                chunk.set_cell(pos, CellType::Revealed);
                                /*if let Some(c2) = self.world.get_mut(&chunk_pos.south()) {
                                    c2.data = 0xAAAAAAAA;
                                }*/
                                if self.get_neighboring_mines_from_pos((chunk_pos, pos)) == 0 {
                                    self.reveal_empty_connected(ctx, (chunk_pos, pos));
                                }

                                should_update = true;
                            }
                            CellType::Revealed => {
                                let neighbors = Self::get_cell_neighbors((chunk_pos, pos));
                                let mut flag_count = 0;
                                let mut mine_count = 0;
                                for neighbor in neighbors.0 {
                                    if self.get_flag(neighbor) {
                                        flag_count += 1;
                                    }
                                    if let CellType::Mine = self.get_cell(neighbor) {
                                        mine_count += 1;
                                    }
                                }
                                if flag_count >= mine_count {
                                    // reveal all unknown neighbors
                                    for neighbor in neighbors.0 {
                                        if self.get_flag(neighbor) {
                                            continue;
                                        }
                                        match self.get_cell(neighbor) {
                                            CellType::Mine => println!("loser"),
                                            CellType::Unknown => {
                                                self.set_cell(neighbor, CellType::Revealed);
                                                if !self.has_known_mine_neighbor(neighbor) {
                                                    self.reveal_empty_connected(ctx, neighbor);
                                                }
                                            }
                                            _ => (),
                                        }
                                    }
                                }
                                should_update = true;
                            }
                            _ => {}
                        }
                    } else {
                        println!("how");
                    }
                }
            }
            _ => (),
        }
        if should_update {
            self.update_tile_batch(ctx).unwrap();
        }
    }
    fn update(&mut self, ctx: &mut Context) -> GameResult<()> {
        let mut should_update = false;
        let mut camera_delta_x = 0;
        let mut camera_delta_y = 0;

        if input::mouse::button_pressed(ctx, event::MouseButton::Left) {
            if input::keyboard::is_key_pressed(ctx, event::KeyCode::LControl) {
                let size_actual = (CHUNK_PX_SIZEF * self.zoom) as i16;
                // move camera
                let delta = input::mouse::delta(ctx);
                self.offset_x += delta.x as i16;
                self.offset_y += delta.y as i16;
                // self.offset_y += (delta.y) as i8;
                if self.offset_x.abs() >= size_actual {
                    camera_delta_x = i32::from(self.offset_x / size_actual);
                    self.camera.x += camera_delta_x;
                    self.offset_x %= size_actual;
                    should_update = true;
                }
                if self.offset_y.abs() >= size_actual {
                    camera_delta_y = i32::from(self.offset_y / size_actual);
                    self.camera.y -= camera_delta_y;
                    self.offset_y %= size_actual;
                    should_update = true;
                }
            } else {
                // get chunk mouse over
            }
        }
        if should_update {
            self.explore_new_chunks(ctx, camera_delta_x, camera_delta_y);
            self.update_tile_batch(ctx)?;
        }
        Ok(())
    }
    fn draw(&mut self, ctx: &mut Context) -> GameResult<()> {
        graphics::clear(ctx, Color::RED);
        /*let (x_size, y_size) = graphics::size(ctx);
        let chunk_start_x = self.camera.x;
        let horiz_chunks = (x_size / CHUNK_PX_SIZEF) as i32;
        let chunk_start_y = self.camera.y;
        let vert_chunks = (y_size / CHUNK_PX_SIZEF) as i32;
        for x in -1..=horiz_chunks {
            for y in -1..vert_chunks {
                self.draw_chunk(
                    ctx,
                    ChunkPos {
                        x: x - chunk_start_x,
                        y: y + chunk_start_y,
                    },
                    x * 32 + i32::from(self.offset_x),
                    y * 32 + i32::from(self.offset_y),
                )?;
            }
        }*/
        graphics::draw(
            ctx,
            &self.tile_batch,
            DrawParam::new()
                .dest(graphics::mint::Vector2 {
                    x: self.offset_x as f32,
                    y: self.offset_y as f32,
                })
                .scale(graphics::mint::Vector2 {
                    x: self.zoom,
                    y: self.zoom,
                }),
        )?;

        /*
        let mouse_pos = self.get_mouse_position(ctx);
        let neighbors = InfiniteMinesweeper::get_cell_neighbors(mouse_pos);

        let mut draw_cell = |(chunk, pos): CellPos| -> GameResult<()> {
            let col = pos / Chunk::SIZE;
            let row = pos % Chunk::SIZE;
            let rect = graphics::Mesh::new_rectangle(
                ctx,
                graphics::DrawMode::stroke(2.0),
                Rect {
                    x: ((self.camera.x + chunk.x) as f32 * CHUNK_PX_SIZEF)
                        + (col as f32 * TILE_PX_SIZEF)
                        + (self.offset_x as f32),
                    y: ((chunk.y - self.camera.y) as f32 * CHUNK_PX_SIZEF)
                        + (row as f32 * TILE_PX_SIZEF)
                        + (self.offset_y as f32),
                    w: TILE_PX_SIZEF,
                    h: TILE_PX_SIZEF,
                },
                Color::RED,
            )?;
            graphics::draw(ctx, &rect, (ggez::mint::Point2 { x: 0.0, y: 0.0 },))
        };

        for cell in neighbors.0 {
            draw_cell(cell)?;
        }
        // draw_cell(mouse_pos)?;*/
        
        // let pos = input::mouse::position(ctx);
        // let aligned_x = pos.x as i32 - self.offset_x as i32;
        // let chunk_x = aligned_x / ((CHUNK_PX_SIZEF * self.zoom) as i32);

        // let aligned_y = pos.y as i32 - self.offset_y as i32;
        // let chunk_y = aligned_y / ((CHUNK_PX_SIZEF * self.zoom) as i32);

        // let rect = graphics::Mesh::new_rectangle(
        //     ctx,
        //     graphics::DrawMode::stroke(2.0),
        //     Rect {
        //         x: (chunk_x as f32 * CHUNK_PX_SIZEF * self.zoom) + (self.offset_x as f32),
        //         y: (chunk_y as f32 * CHUNK_PX_SIZEF * self.zoom) + (self.offset_y as f32),
        //         w: CHUNK_PX_SIZEF * self.zoom,
        //         h: CHUNK_PX_SIZEF * self.zoom,
        //     },
        //     Color::RED,
        // )?;
        // graphics::draw(ctx, &rect, (ggez::mint::Point2 { x: 0.0, y: 0.0 },))?;
        graphics::present(ctx)?;
        Ok(())
    }

    fn quit_event(&mut self, _ctx: &mut Context) -> bool {
        self.write_save();
        false
    }
}
