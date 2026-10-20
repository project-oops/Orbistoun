//! The format's constant tables, as LibAtrac9 (MIT, ACKNOWLEDGEMENTS.md) lists them in
//! `C/src/tables.c`, `C/src/bit_allocation.c` and `C/src/band_extension.c`.

#![allow(
    clippy::unreadable_literal,
    clippy::excessive_precision,
    reason = "values transcribed digit for digit, so each can be checked against the reference"
)]

/// What kind of block a channel configuration holds, and so how many channels it codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BlockKind {
    /// One channel.
    Mono,
    /// Two channels, sharing band parameters and coding the upper bands once.
    Stereo,
    /// One low-frequency channel, two quantisation units wide.
    Lfe,
}

impl BlockKind {
    /// How many channels a block of this kind codes.
    pub(crate) const fn channels(self) -> usize {
        match self {
            Self::Stereo => 2,
            Self::Mono | Self::Lfe => 1,
        }
    }
}

/// The blocks of each channel configuration, by the configuration word's index (`tables.c`
/// 9-17).
pub(crate) const CHANNEL_CONFIGS: [&[BlockKind]; 6] = {
    use BlockKind::{Lfe, Mono, Stereo};
    [
        &[Mono],
        &[Mono, Mono],
        &[Stereo],
        &[Stereo, Mono, Lfe, Stereo],
        &[Stereo, Mono, Lfe, Stereo, Stereo],
        &[Stereo, Stereo],
    ]
};

/// The highest precision coded through a Huffman codebook, by whether the sample rate is a high
/// one (`tables.c` 19).
pub(crate) const MAX_HUFF_PRECISION: [i32; 2] = [7, 1];

/// The fewest bands a block codes, by whether the sample rate is a high one (`tables.c` 20).
pub(crate) const MIN_BAND_COUNT: [usize; 2] = [3, 1];

/// The highest band band extension may reach, by whether the sample rate is a high one
/// (`tables.c` 21).
pub(crate) const MAX_EXTENSION_BAND: [usize; 2] = [18, 16];

/// Each sample rate index's frame length, as a power of two (`tables.c` 23-24).
pub(crate) const FRAME_SAMPLES_POWER: [u32; 16] = [6, 6, 7, 7, 7, 8, 8, 8, 6, 6, 7, 7, 7, 8, 8, 8];

/// The most bands each sample rate index allows (`tables.c` 26-27).
pub(crate) const MAX_BAND_COUNT: [usize; 16] =
    [8, 8, 12, 12, 12, 18, 18, 18, 8, 8, 12, 12, 12, 16, 16, 16];

/// How many quantisation units the first `n` bands span (`tables.c` 29-30).
pub(crate) const BAND_TO_UNIT_COUNT: [usize; 19] = [
    0, 4, 8, 10, 12, 13, 14, 15, 16, 18, 20, 21, 22, 23, 24, 25, 26, 28, 30,
];

/// How many spectral coefficients each quantisation unit holds (`tables.c` 32-36).
pub(crate) const UNIT_COEFF_COUNT: [usize; 30] = [
    2, 2, 2, 2, 2, 2, 2, 2, 4, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 16, 16, 16, 16, 16, 16, 16, 16, 16,
    16,
];

/// The first spectral coefficient of each quantisation unit, and the end of the last
/// (`tables.c` 38-42).
pub(crate) const UNIT_COEFF_INDEX: [usize; 31] = [
    0, 2, 4, 6, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64, 72, 80, 88, 96, 112, 128, 144,
    160, 176, 192, 208, 224, 240, 256,
];

/// Which of the four codebooks of a precision each quantisation unit is coded with
/// (`tables.c` 44-48).
pub(crate) const UNIT_CODEBOOK_INDEX: [usize; 30] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
];

/// The sample rate of each index, in Hz (`tables.c` 50-54).
pub(crate) const SAMPLE_RATES: [u32; 16] = [
    11_025, 12_000, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000, 44_100, 48_000, 64_000, 88_200,
    96_000, 128_000, 176_400, 192_000,
];

/// The weights subtracted from scale factors coded as offsets, by the weight set a channel names
/// (`tables.c` 56-73).
pub(crate) const SCALE_FACTOR_WEIGHTS: [[i32; 32]; 8] = [
    [
        0, 0, 0, 1, 1, 2, 2, 2, 2, 2, 2, 3, 2, 3, 3, 4, 4, 4, 4, 4, 4, 5, 5, 6, 6, 7, 7, 8, 10, 12,
        12, 12,
    ],
    [
        3, 2, 2, 1, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 2, 3, 3, 4, 5, 7, 10,
        10, 10,
    ],
    [
        0, 2, 4, 5, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 7, 7, 7, 7, 8, 9, 12,
        12, 12,
    ],
    [
        0, 1, 1, 2, 2, 2, 3, 3, 3, 3, 3, 4, 4, 4, 5, 5, 5, 6, 6, 6, 6, 7, 8, 8, 10, 11, 11, 12, 13,
        13, 13, 13,
    ],
    [
        0, 2, 2, 3, 3, 4, 4, 5, 4, 5, 5, 5, 5, 6, 7, 8, 8, 8, 8, 9, 9, 9, 10, 10, 11, 12, 12, 13,
        13, 14, 14, 14,
    ],
    [
        1, 1, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 3, 3, 3, 4, 4, 5, 6, 7, 7, 9, 11,
        11, 11,
    ],
    [
        0, 5, 8, 10, 11, 11, 12, 12, 12, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13,
        13, 12, 12, 12, 12, 13, 15, 15, 15,
    ],
    [
        0, 2, 3, 4, 5, 6, 6, 7, 7, 8, 8, 8, 9, 9, 10, 10, 10, 11, 11, 11, 11, 11, 11, 12, 12, 12,
        12, 13, 13, 15, 15, 15,
    ],
];

/// The step size of each coarse precision, `2 / (2^p - 1)` (`tables.c` 93-99).
pub(crate) const QUANTIZER_STEP: [f64; 16] = [
    2.0000000000000000e+0,
    6.6666666666666663e-1,
    2.8571428571428570e-1,
    1.3333333333333333e-1,
    6.4516129032258063e-2,
    3.1746031746031744e-2,
    1.5748031496062992e-2,
    7.8431372549019607e-3,
    3.9138943248532287e-3,
    1.9550342130987292e-3,
    9.7703957010258913e-4,
    4.8840048840048840e-4,
    2.4417043096081065e-4,
    1.2207776353537203e-4,
    6.1037018951994385e-5,
    3.0518043793392844e-5,
];

/// The step size of each fine precision (`tables.c` 101-107).
pub(crate) const QUANTIZER_FINE_STEP: [f64; 16] = [
    3.0518043793392844e-05,
    1.0172681264464281e-05,
    4.3597205419132631e-06,
    2.0345362528928561e-06,
    9.8445302559331759e-07,
    4.8441339354591809e-07,
    2.4029955742829012e-07,
    1.1967860311134448e-07,
    5.9722199204291275e-08,
    2.9831909866464167e-08,
    1.4908668194134265e-08,
    7.4525137468602791e-09,
    3.7258019525568114e-09,
    1.8627872668859698e-09,
    9.3136520869755679e-10,
    4.6567549848772173e-10,
];

/// The scale of each scale factor: `2^(sf - 15)` (`tables.c` 75-85).
pub(crate) fn spectrum_scale(scale_factor: i32) -> f64 {
    2.0_f64.powi(scale_factor - 15)
}

/// The curve every bit-allocation gradient is drawn from (`bit_allocation.c` 123-128).
pub(crate) const BASE_CURVE: [i32; 48] = [
    1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 4, 4, 5, 5, 6, 7, 8, 9, 10, 11, 12, 13, 15, 16, 18, 19, 20,
    21, 22, 23, 24, 25, 26, 26, 27, 27, 28, 28, 28, 29, 29, 29, 29, 30, 30, 30, 30,
];

/// For a block coding `13 + i` quantisation units with band extension: the units the second and
/// third extension groups start at, and how many bands it codes (`band_extension.c` 210-220).
pub(crate) const BEX_GROUPS: [(usize, usize, usize); 8] = [
    (16, 21, 0),
    (18, 22, 1),
    (20, 22, 2),
    (21, 22, 3),
    (21, 22, 3),
    (23, 24, 4),
    (23, 24, 4),
    (24, 24, 5),
];

/// How many values band extension codes, by mode and band count (`band_extension.c` 222-229).
pub(crate) const BEX_VALUE_COUNTS: [[usize; 6]; 5] = [
    [0, 0, 0, 4, 4, 2],
    [0, 0, 0, 0, 0, 0],
    [0, 0, 0, 2, 2, 1],
    [0, 0, 0, 2, 2, 2],
    [1, 1, 1, 0, 0, 0],
];

/// Each band-extension value's width in bits, by mode, band count and value
/// (`band_extension.c` 232-270).
pub(crate) const BEX_DATA_LENGTHS: [[[u32; 4]; 6]; 5] = [
    [
        [0; 4],
        [0; 4],
        [0; 4],
        [5, 4, 3, 3],
        [4, 4, 3, 4],
        [4, 5, 0, 0],
    ],
    [[0; 4]; 6],
    [
        [0; 4],
        [0; 4],
        [0; 4],
        [6, 6, 0, 0],
        [6, 6, 0, 0],
        [6, 0, 0, 0],
    ],
    [
        [0; 4],
        [0; 4],
        [0; 4],
        [4, 4, 0, 0],
        [4, 4, 0, 0],
        [4, 4, 0, 0],
    ],
    [
        [3, 0, 0, 0],
        [3, 0, 0, 0],
        [3, 0, 0, 0],
        [0; 4],
        [0; 4],
        [0; 4],
    ],
];

/// Mode 0's scales for three bands, by group and coded value (`band_extension.c` 272-304); the
/// last three rows have fewer than 32 entries in the reference and are zero past them.
pub(crate) const BEX_MODE0_BANDS3: [[f64; 32]; 5] = [
    [
        0.000000e+0,
        1.988220e-1,
        2.514343e-1,
        2.960510e-1,
        3.263550e-1,
        3.771362e-1,
        3.786926e-1,
        4.540405e-1,
        4.877625e-1,
        5.262451e-1,
        5.447083e-1,
        5.737000e-1,
        6.212158e-1,
        6.222839e-1,
        6.560974e-1,
        6.896667e-1,
        7.555542e-1,
        7.677917e-1,
        7.918091e-1,
        7.971497e-1,
        8.188171e-1,
        8.446045e-1,
        9.790649e-1,
        9.822083e-1,
        9.846191e-1,
        9.859314e-1,
        9.863586e-1,
        9.863892e-1,
        9.873352e-1,
        9.881287e-1,
        9.898682e-1,
        9.913330e-1,
    ],
    [
        0.000000e+0,
        9.982910e-1,
        7.592773e-2,
        7.179565e-1,
        9.851379e-1,
        5.340271e-1,
        9.013672e-1,
        6.349182e-1,
        7.226257e-1,
        1.948547e-1,
        7.628174e-1,
        9.873657e-1,
        8.112183e-1,
        2.715454e-1,
        9.734192e-1,
        1.443787e-1,
        4.640198e-1,
        3.249207e-1,
        3.790894e-1,
        8.276367e-2,
        5.954590e-1,
        2.864380e-1,
        9.806824e-1,
        7.929077e-1,
        6.292114e-1,
        4.887085e-1,
        2.905273e-1,
        1.301880e-1,
        3.140869e-1,
        5.482483e-1,
        4.210815e-1,
        1.182861e-1,
    ],
    [
        0.000000e+0,
        3.155518e-2,
        8.581543e-2,
        1.364746e-1,
        1.858826e-1,
        2.368469e-1,
        2.888184e-1,
        3.432617e-1,
        4.012451e-1,
        4.623108e-1,
        5.271301e-1,
        5.954895e-1,
        6.681213e-1,
        7.448425e-1,
        8.245239e-1,
        9.097290e-1,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
    ],
    [
        0.000000e+0,
        4.418945e-2,
        1.303711e-1,
        2.273560e-1,
        3.395996e-1,
        4.735718e-1,
        6.267090e-1,
        8.003845e-1,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
    ],
    [
        0.000000e+0,
        2.804565e-2,
        9.683228e-2,
        1.849976e-1,
        3.005981e-1,
        4.470520e-1,
        6.168518e-1,
        8.007813e-1,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
    ],
];

/// Mode 0's scales for four bands (`band_extension.c` 306-332); short rows are zero past their
/// end.
pub(crate) const BEX_MODE0_BANDS4: [[f64; 16]; 5] = [
    [
        0.000000e+0,
        2.708740e-1,
        3.479614e-1,
        3.578186e-1,
        5.083618e-1,
        5.299072e-1,
        5.819092e-1,
        6.381836e-1,
        7.276917e-1,
        7.595520e-1,
        7.878723e-1,
        9.707336e-1,
        9.713135e-1,
        9.736023e-1,
        9.759827e-1,
        9.832458e-1,
    ],
    [
        0.000000e+0,
        2.330627e-1,
        5.891418e-1,
        7.170410e-1,
        2.036438e-1,
        1.613464e-1,
        6.668701e-1,
        9.481201e-1,
        9.769897e-1,
        5.111694e-1,
        3.522644e-1,
        8.209534e-1,
        2.933960e-1,
        9.757690e-1,
        5.289917e-1,
        4.372253e-1,
    ],
    [
        0.000000e+0,
        4.360962e-2,
        1.056519e-1,
        1.590576e-1,
        2.078857e-1,
        2.572937e-1,
        3.082581e-1,
        3.616028e-1,
        4.191589e-1,
        4.792175e-1,
        5.438538e-1,
        6.125183e-1,
        6.841125e-1,
        7.589417e-1,
        8.365173e-1,
        9.148254e-1,
    ],
    [
        0.000000e+0,
        4.074097e-2,
        1.164551e-1,
        2.077026e-1,
        3.184509e-1,
        4.532166e-1,
        6.124268e-1,
        7.932129e-1,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
    ],
    [
        0.000000e+0,
        8.880615e-3,
        2.932739e-2,
        5.593872e-2,
        8.825684e-2,
        1.259155e-1,
        1.721497e-1,
        2.270813e-1,
        2.901611e-1,
        3.579712e-1,
        4.334106e-1,
        5.147095e-1,
        6.023254e-1,
        6.956177e-1,
        7.952881e-1,
        8.977356e-1,
    ],
];

/// Mode 0's scales for five bands (`band_extension.c` 334-360); the first row is zero past its
/// sixteenth entry.
pub(crate) const BEX_MODE0_BANDS5: [[f64; 32]; 3] = [
    [
        0.000000e+0,
        7.379150e-2,
        1.806335e-1,
        2.687073e-1,
        3.407898e-1,
        4.047546e-1,
        4.621887e-1,
        5.168762e-1,
        5.703125e-1,
        6.237488e-1,
        6.763611e-1,
        7.288208e-1,
        7.808533e-1,
        8.337708e-1,
        8.874512e-1,
        9.418030e-1,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
    ],
    [
        0.000000e+0,
        7.980347e-2,
        1.615295e-1,
        1.665649e-1,
        1.822205e-1,
        2.185669e-1,
        2.292175e-1,
        2.456665e-1,
        2.666321e-1,
        3.306580e-1,
        3.330688e-1,
        3.765259e-1,
        4.085083e-1,
        4.400024e-1,
        4.407654e-1,
        4.817505e-1,
        4.924011e-1,
        5.320740e-1,
        5.893860e-1,
        6.131287e-1,
        6.212463e-1,
        6.278076e-1,
        6.308899e-1,
        7.660828e-1,
        7.850647e-1,
        7.910461e-1,
        7.929382e-1,
        8.038330e-1,
        9.834900e-1,
        9.846191e-1,
        9.852295e-1,
        9.862671e-1,
    ],
    [
        0.000000e+0,
        6.084290e-1,
        3.672791e-1,
        3.151855e-1,
        1.488953e-1,
        2.571716e-1,
        5.103455e-1,
        3.311157e-1,
        5.426025e-2,
        4.254456e-1,
        7.998352e-1,
        7.873230e-1,
        5.418701e-1,
        2.925110e-1,
        8.468628e-2,
        1.410522e-1,
        9.819641e-1,
        9.609070e-1,
        3.530884e-2,
        9.729004e-2,
        5.758362e-1,
        9.941711e-1,
        7.215576e-1,
        7.183228e-1,
        2.028809e-1,
        9.588623e-2,
        2.032166e-1,
        1.338806e-1,
        5.003357e-1,
        1.874390e-1,
        9.804993e-1,
        1.107788e-1,
    ],
];

/// Mode 2's scales (`band_extension.c` 362-380).
pub(crate) const BEX_MODE2_SCALE: [f64; 64] = [
    4.272461e-4,
    1.312256e-3,
    2.441406e-3,
    3.692627e-3,
    4.913330e-3,
    6.134033e-3,
    7.507324e-3,
    8.972168e-3,
    1.049805e-2,
    1.223755e-2,
    1.406860e-2,
    1.599121e-2,
    1.800537e-2,
    2.026367e-2,
    2.264404e-2,
    2.517700e-2,
    2.792358e-2,
    3.073120e-2,
    3.344727e-2,
    3.631592e-2,
    3.952026e-2,
    4.275513e-2,
    4.608154e-2,
    4.968262e-2,
    5.355835e-2,
    5.783081e-2,
    6.195068e-2,
    6.677246e-2,
    7.196045e-2,
    7.745361e-2,
    8.319092e-2,
    8.993530e-2,
    9.759521e-2,
    1.056213e-1,
    1.138916e-1,
    1.236267e-1,
    1.348267e-1,
    1.470337e-1,
    1.603394e-1,
    1.755676e-1,
    1.905823e-1,
    2.071228e-1,
    2.245178e-1,
    2.444153e-1,
    2.658997e-1,
    2.897644e-1,
    3.146057e-1,
    3.450012e-1,
    3.766174e-1,
    4.122620e-1,
    4.505615e-1,
    4.893799e-1,
    5.305481e-1,
    5.731201e-1,
    6.157837e-1,
    6.580811e-1,
    6.985168e-1,
    7.435303e-1,
    7.865906e-1,
    8.302612e-1,
    8.718567e-1,
    9.125671e-1,
    9.575806e-1,
    9.996643e-1,
];

/// Mode 3's starting scales (`band_extension.c` 382-388).
pub(crate) const BEX_MODE3_INITIAL: [f64; 16] = [
    3.491211e-1,
    5.371094e-1,
    6.782227e-1,
    7.910156e-1,
    9.057617e-1,
    1.024902e+0,
    1.156250e+0,
    1.290527e+0,
    1.458984e+0,
    1.664551e+0,
    1.929688e+0,
    2.278320e+0,
    2.831543e+0,
    3.659180e+0,
    5.257813e+0,
    8.373047e+0,
];

/// Mode 3's per-coefficient rates, as powers of two (`band_extension.c` 390-396).
pub(crate) const BEX_MODE3_RATE: [f64; 16] = [
    -2.913818e-1,
    -2.541504e-1,
    -1.664429e-1,
    -1.476440e-1,
    -1.342163e-1,
    -1.220703e-1,
    -1.117554e-1,
    -1.026611e-1,
    -9.436035e-2,
    -8.483887e-2,
    -7.476807e-2,
    -6.304932e-2,
    -4.492188e-2,
    -2.447510e-2,
    1.831055e-4,
    4.174805e-2,
];

/// Mode 4's multipliers (`band_extension.c` 398-402).
pub(crate) const BEX_MODE4_MULTIPLIER: [f64; 8] = [
    3.610229e-2,
    1.260681e-1,
    2.227478e-1,
    3.338318e-1,
    4.662170e-1,
    6.221313e-1,
    7.989197e-1,
    9.939575e-1,
];
