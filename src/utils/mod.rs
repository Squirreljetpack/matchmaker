pub mod text;

// macro_rules! impl_convert_for_deserialize {
//     ($src:ty => $dst:ty, { $($field:ident),* $(,)? }) => {
//         impl From<$src> for $dst {
//             fn from(s: $src) -> Self {
//                 Self {
//                     $($field: s.$field),*
//                 }
//             }
//         }

//         paste::paste! {
//             fn [<deserialize_ $dst:lower>]< 'de, D>(deserializer: D ) -> Result<$dst, D::Error>
//             where
//             D: serde::Deserializer<'de>,
//             {
//                 let s = $src::deserialize(deserializer)?;
//                 Ok(s.into())
//             }
//         }
//     };
// }
// #[derive(Deserialize)]
// struct PaddingSetting {
//     pub left: u16,
//     pub right: u16,
//     pub top: u16,
//     pub bottom: u16,
// }

// impl_convert_for_deserialize!(PaddingSetting => Padding, { left, right, top, bottom });