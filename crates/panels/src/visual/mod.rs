pub mod financial;
pub mod scientific;
pub mod realtime;
pub mod hierarchy;
pub mod network;
pub mod specialized;

// Backward compatibility re-exports (old paths still work)
pub use financial::depth_chart;
pub use scientific::space3d;

// Convenience re-exports for all modules
pub use financial::{volatility_surface, yield_curve, treemap, order_flow_heatmap, dom_surface, liquidation_heatmap, pnl_surface, horizon_chart, calendar_heatmap};
pub use scientific::{point_cloud, contour_plot, isosurface, phase_space, wavelet_viz, parallel_coordinates};
pub use realtime::{particle_system, force_graph, flow_field, gpu_timeseries, streaming_heatmap};
pub use hierarchy::{sunburst, icicle, circular_packing};
pub use network::{sankey, chord, alluvial, blockchain_graph};
pub use specialized::{flame_graph, stream_graph, radar_chart, bubble_chart};
