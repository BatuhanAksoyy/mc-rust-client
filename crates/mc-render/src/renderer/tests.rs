use super::*;

#[test]
fn empty_mesh_halves_skip_slicing_before_upload_and_after_clear() {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let empty = || {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &[],
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        })
    };
    let triangle = [Vertex::zeroed(); 3];
    for halves in [
        [&[][..], &[][..]],
        [&triangle[..], &[][..]],
        [&[][..], &triangle[..]],
        [&triangle[..], &triangle[..]],
    ] {
        for vertices in halves {
            let buffer = empty();
            assert_eq!(buffer.size(), 0);
            assert!(vertex_slice(&buffer, 0).is_none());
            let (buffer, count) = Renderer::upload(&device, &queue, &buffer, "test", vertices);
            assert_eq!(vertex_slice(&buffer, count).is_some(), !vertices.is_empty());
            let capacity = buffer.size();
            let (buffer, count) = Renderer::upload(&device, &queue, &buffer, "test", &[]);
            assert_eq!(buffer.size(), capacity);
            assert!(vertex_slice(&buffer, count).is_none());
        }
    }
}
