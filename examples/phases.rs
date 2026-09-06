//! Memory used in phases and thrown away together, which is what a bump
//! allocator is for.
//!
//! A frame of a simulation takes what it needs out of an arena, does its work,
//! and the arena is reset for the next frame. Nothing is returned one block at
//! a time, because nothing needs to be: the frame is the unit.
//!
//! ```text
//! cargo run --example phases
//! ```

use core::alloc::Layout;

use dollop::{Bump, Strategy};

/// One particle, as a frame sees it.
#[repr(C)]
#[derive(Clone, Copy)]
struct Particle {
    x:  f32,
    y:  f32,
    vx: f32,
    vy: f32,
}

fn main() {
    let mut region = [0u8; 4096];
    let mut arena = Bump::new(&mut region);
    println!("a {} byte arena\n", arena.free_bytes());

    let layout = Layout::array::<Particle>(64).expect("a valid layout");

    for frame in 0 .. 3 {
        // Everything this frame needs, out of the arena. An add and a compare
        // per block, and no header written anywhere.
        let particles = arena
            .allocate(layout)
            .expect("the arena holds a frame")
            .cast::<Particle>();
        let scratch = arena
            .allocate(Layout::array::<f32>(64).expect("a valid layout"))
            .expect("and some scratch")
            .cast::<f32>();

        // SAFETY: both blocks are the size their layouts say and nothing else
        // holds them, so writing every slot and reading it back is in bounds.
        let energy: f32 = unsafe {
            for i in 0 .. 64 {
                let v = (i as f32 + frame as f32) * 0.5;
                particles.as_ptr().add(i).write(Particle {
                    x:  v,
                    y:  v,
                    vx: 1.0,
                    vy: -1.0,
                });
                scratch.as_ptr().add(i).write(v * v);
            }
            (0 .. 64).map(|i| scratch.as_ptr().add(i).read()).sum()
        };

        println!(
            "frame {frame}: {} bytes used, energy {energy:.1}",
            arena.used_bytes()
        );

        // The frame is over. Nothing is returned block by block; the mark goes
        // back to the start and the next frame starts from there.
        //
        // SAFETY: nothing from this frame is held past this line.
        unsafe { arena.reset() };
    }

    println!(
        "\nafter the last reset, {} bytes free, the whole arena",
        arena.free_bytes()
    );
}
