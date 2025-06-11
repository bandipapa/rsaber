// Use the following settings when exporting from Blender:
// - Forward Axis: Y, Up Axis: Z
//   z
//   ^ y
//   |/
//   +--> x
// - Geometry: Normals, Triangulated Mesh, Apply Modifiers

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::iter;

use wgpu::{BufferUsages, Device};
use wgpu::util::{BufferInitDescriptor, DeviceExt};

use crate::AssetManagerRc;
use crate::model::{InstShaderType, Mesh, PrimitiveStateType, Submesh, VertexPosNormal, VertexShaderType};

type MeshIndex = u32;

pub struct Obj;

impl Obj {
    pub fn open<S: AsRef<str>>(asset_mgr: AssetManagerRc, device: &Device, name: S, submesh_infos: &[(&str, &InstShaderType)]) -> Mesh { // TODO: convert it to func?
        assert!(!submesh_infos.is_empty());

        // Parse obj.

        let mut reader = BufReader::new(asset_mgr.open(&format!("obj/{}.obj", name.as_ref())));

        let mut submeshes: Box<[_]> = iter::repeat_with(|| None).take(submesh_infos.len()).collect(); // iter::repeat can'be used since Submesh doesn't implement Clone.
        let mut inst_index_opt = None;

        let mut base_pos: MeshIndex = 0;
        let mut base_normal: MeshIndex = 0;
        let mut poss = Vec::new();
        let mut normals = Vec::new();
        let mut index_map = HashMap::new();

        let mut base_vertex: MeshIndex = 0;
        let mut base_index: MeshIndex = 0;
        let mut vertexes = Vec::new();
        let mut indexes = Vec::new();

        loop {
            let mut finish = || { // TODO: Move to outside of the loop?
                if let Some(inst_index) = inst_index_opt {
                    let submesh_info: &(&str, &InstShaderType) = &submesh_infos[inst_index]; // TODO: Why do we need type annotation here?
                    let submesh = Submesh::new(base_index, indexes.len().try_into().unwrap(), base_vertex.try_into().unwrap(), PrimitiveStateType::TriangleList, submesh_info.1.clone());
                    submeshes[inst_index] = Some(submesh);
                    inst_index_opt = None;
                }
            };

            let mut buf = String::new();

            let r = reader.read_line(&mut buf).unwrap();
            if r == 0 { // EOF
                finish();
                break;
            }

            let line = buf.trim_end();
            let (op, line) = line.split_once(' ').unwrap();

            if op == "o" {
                finish();

                base_pos += TryInto::<MeshIndex>::try_into(poss.len()).unwrap();
                base_normal += TryInto::<MeshIndex>::try_into(normals.len()).unwrap();
                base_vertex = TryInto::<MeshIndex>::try_into(vertexes.len()).unwrap();
                base_index = TryInto::<MeshIndex>::try_into(indexes.len()).unwrap();

                poss.clear();
                normals.clear();
                index_map.clear();

                if let Some((i, _)) = submesh_infos.iter().enumerate().find(|(_, submesh_info)| submesh_info.0 == line) {
                    assert!(submeshes[i].is_none()); // It is forbidden to have duplicated object names.
                    inst_index_opt = Some(i);
                }
            } else if inst_index_opt.is_some() {
                match op {
                    "v" => { // v -0.350000 -0.350000 -0.500000
                        let nums: Box<[f32]> = line.split(' ').map(|num| num.parse().unwrap()).collect();
                        assert!(nums.len() == 3);
                        poss.push([nums[0], nums[1], nums[2]]);
                    },
                    "vn" => { // vn 0.1280 -0.9835 0.1280
                        let nums: Box<[f32]> = line.split(' ').map(|num| num.parse().unwrap()).collect();
                        assert!(nums.len() == 3);
                        normals.push([nums[0], nums[1], nums[2]]);
                    },
                    "f" => { // f 62//62 2//2 51//51
                        let face_indexes: Box<[_]> = line.split(' ').collect();
                        assert!(face_indexes.len() == 3);

                        for face_index in face_indexes {
                            let nums: Box<[_]> = face_index.split('/').collect();
                            assert!(nums.len() == 3);

                            let nums: Box<[_]> = [nums[0], nums[2]].iter().map(|num| num.parse::<MeshIndex>().unwrap() - 1).collect();
                            let (pos_index, normal_index) = (nums[0] - base_pos, nums[1] - base_normal);

                            let index_key = (pos_index, normal_index);
                            let index = if let Some(index) = index_map.get(&index_key) {
                                *index
                            } else {
                                let index: u16 = (vertexes.len() - base_vertex as usize).try_into().unwrap(); // Submesh index is u16, see ModelRenderer->render->set_index_buffer().
                                index_map.insert(index_key, index);

                                let pos = poss[pos_index as usize];
                                let normal = normals[normal_index as usize];

                                let vertex = VertexPosNormal {
                                    pos,
                                    normal,
                                };

                                vertexes.push(vertex);
                                index
                            };
                            
                            indexes.push(index);
                        }
                    },
                    _ => (),
                }
            } else {
                match op {
                    "v" => base_pos += 1,
                    "vn" => base_normal += 1,
                    _ => (),
                }
            }
        }

        // Create buffers.

        let vertex_buf = device.create_buffer_init(&BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&vertexes),
            usage: BufferUsages::VERTEX,
        });

        let index_buf = device.create_buffer_init(&BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&indexes),
            usage: BufferUsages::INDEX,
        });

        let submeshes = submeshes.into_iter().map(|submesh| submesh.expect("Incomplete mesh")).collect();

        Mesh::new(vertex_buf, index_buf, VertexShaderType::PosNormal, submeshes)
    }
}
