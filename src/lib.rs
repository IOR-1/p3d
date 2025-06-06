#![no_std]

#[macro_use]
extern crate alloc;
#[macro_use]
extern crate ndarray;

use alloc::string::String;
use alloc::vec::Vec;
use gltf::{Gltf, Semantic};

use obj::{load_obj, Obj, Vertex, ObjError};
use tri_mesh::prelude::*;
use cgmath::Point2;
use ndarray::arr2;
use ndarray::Array3;
use tri_mesh::mesh_builder::Error as MeshError;
use crate::algo_grid::{get_contour, intersect, intersect_2};
use crate::contour::Rect;

mod polyline;
mod contour;
mod algo_grid;
use algo_grid::{
    find_top_std,
    find_top_std_2,
    find_top_std_3,
    find_top_std_4,
};
type Vec2 = Point2<f64>;


#[derive(Debug)]
pub enum AlgoType {
    Grid2d,
    Grid2dV2,
    Grid2dV3,
    Grid2dV3a,
    Spectr,
}

#[derive(Debug)]
pub enum InputFileType {
    Obj,
    Gltf,
    Glb,
}

#[derive(Debug)]
pub enum P3DError {
    InvalidObject(ObjError),
    MeshError(MeshError),
    MathError,
    UnsupportedFileType,
    GltfError(String),
}


pub fn p3d_process(input: &[u8], file_type: InputFileType, algo: AlgoType, par1: i16, par2: i16, trans: Option<[u8;4]>) -> Result<Vec<String>, P3DError> {
    p3d_process_n(input, file_type, algo, 10, par1, par2, trans)
}

#[allow(unused_variables)]
pub fn p3d_process_n(input: &[u8], file_type: InputFileType, algo: AlgoType, depth: usize, par1: i16, par2: i16, trans: Option<[u8;4]>) -> Result<Vec<String>, P3DError>
{
    let grid_size: i16 = par1;
    let n_sections: i16 = par2;

    let (model_vertices, model_indices): (Vec<f64>, Vec<u32>) = match file_type {
        InputFileType::Obj => {
            let model: Obj<Vertex, u32> = load_obj(input).map_err(|e| P3DError::InvalidObject(e))?;
            let vertices = model.vertices
                .iter()
                .flat_map(|v| v.position.iter())
                .map(|v| <f64 as NumCast>::from(*v).unwrap())
                .collect();
            (vertices, model.indices)
        }
        InputFileType::Gltf => {
            // TODO: Implement glTF loading
            let gltf_data = Gltf::from_slice(input).map_err(|e| P3DError::GltfError(format!("glTF parsing error: {:?}", e)))?;
            let mut positions: Vec<[f32; 3]> = Vec::new();
            let mut indices: Vec<u32> = Vec::new();

            for mesh in gltf_data.meshes() {
                for primitive in mesh.primitives() {
                    let reader = primitive.reader(|buffer| Some(gltf_data.blob.as_deref().unwrap_or(&buffer.source()[..])));
                    if let Some(pos_iter) = reader.read_positions() {
                        positions.extend(pos_iter);
                    }
                    if let Some(indices_iter) = reader.read_indices() {
                        indices.extend(indices_iter.into_u32());
                    }
                    if !positions.is_empty() && !indices.is_empty() {
                        break;
                    }
                }
                if !positions.is_empty() && !indices.is_empty() {
                    break;
                }
            }

            if positions.is_empty() || indices.is_empty() {
                return Err(P3DError::GltfError("No valid geometry (vertices/indices) found in glTF file".to_string()));
            }

            let model_vertices_f64: Vec<f64> = positions.into_iter().flat_map(|pos| [pos[0] as f64, pos[1] as f64, pos[2] as f64]).collect();
            (model_vertices_f64, indices)
        }
        InputFileType::Glb => {
            // TODO: Implement GLB loading
            let gltf_data = Gltf::from_slice(input).map_err(|e| P3DError::GltfError(format!("GLB parsing error: {:?}", e)))?;
            let mut positions: Vec<[f32; 3]> = Vec::new();
            let mut indices: Vec<u32> = Vec::new();

            for mesh in gltf_data.meshes() {
                for primitive in mesh.primitives() {
                    let reader = primitive.reader(|buffer| Some(gltf_data.blob.as_deref().unwrap_or(&buffer.source()[..])));
                    if let Some(pos_iter) = reader.read_positions() {
                        positions.extend(pos_iter);
                    }
                    if let Some(indices_iter) = reader.read_indices() {
                        indices.extend(indices_iter.into_u32());
                    }
                    if !positions.is_empty() && !indices.is_empty() {
                        break;
                    }
                }
                if !positions.is_empty() && !indices.is_empty() {
                    break;
                }
            }

            if positions.is_empty() || indices.is_empty() {
                return Err(P3DError::GltfError("No valid geometry (vertices/indices) found in GLB file".to_string()));
            }

            let model_vertices_f64: Vec<f64> = positions.into_iter().flat_map(|pos| [pos[0] as f64, pos[1] as f64, pos[2] as f64]).collect();
            (model_vertices_f64, indices)
        }
    };

    let mut mesh = MeshBuilder::new()
        .with_indices(model_indices)
        .with_positions(model_vertices)
        .build()
        .map_err(|e| P3DError::MeshError(e))?;

    let mut triangles: Array3<f64> = Array3::zeros((mesh.no_faces(), 3, 3));

    for (i, fid) in mesh.face_iter().enumerate() {
        let vs = mesh.face_vertices(fid);
        let v1 = mesh.vertex_position(vs.0);
        let v2 = mesh.vertex_position(vs.1);
        let v3 = mesh.vertex_position(vs.2);
        triangles.slice_mut(s![i, .., ..])
            .assign(
                &arr2(&[
                    [v1.x as f64, v1.y as f64, v1.z as f64],
                    [v2.x as f64, v2.y as f64, v2.z as f64],
                    [v3.x as f64, v3.y as f64, v3.z as f64],
                ]
                ));
    }

    let pit = algo_grid::principal_inertia_transform(triangles);

    let a: Matrix3<f64> = Matrix3::new(
        pit[[0, 0]], pit[[0, 1]], pit[[0, 2]],
        pit[[1, 0]], pit[[1, 1]], pit[[1, 2]],
        pit[[2, 0]], pit[[2, 1]], pit[[2, 2]],
    );

    let b = a.invert().ok_or(P3DError::MathError)?;

    let tr: Matrix4<f64> = Matrix4::new(
        b.x[0], b.x[1], b.x[2], 0.0,
        b.y[0], b.y[1], b.y[2], 0.0,
        b.z[0], b.z[1], b.z[2], 0.0,
        0.0, 0.0, 0.0, 1.0,
    );

    let shift = Vector3::new(pit[[0, 3]], pit[[1, 3]], pit[[2, 3]]);

    mesh.translate(shift);
    mesh.apply_transformation(tr);

    let k = 45.0 / 256.0;
    if let Some(rot) = trans {
        let axis_normalized = Vector3::new(
            rot[0] as f64 * k,
            rot[1] as f64 * k,
            rot[2] as f64 * k,
        ).normalize();
        mesh.apply_transformation(
            Mat4::from_axis_angle(
                axis_normalized,
                Deg(rot[3] as f64 * k * 360.0 / 256.0),
            )
        );
    }
    let (v_min, v_max) = mesh.extreme_coordinates();

    let mut centers: Vec<Vec<Vec2>> = Vec::with_capacity(depth);
    let step = (v_max.z - v_min.z) / (1.0f64 + n_sections as f64);
    for n in 0..n_sections {
        let z_sect = v_min.z + (n as f64 + 1.0f64) * step;
        let sect = if let AlgoType::Grid2dV3a = algo {
            intersect_2(&mesh, z_sect, step * 0.01)
        } else {
            intersect(&mesh, z_sect)
        };
        let cntr = get_contour(sect);
        if cntr.len() > 0 {
            centers.push(cntr);
        }
    }
    let rect = Rect::new(v_min.x, v_max.x, v_min.y, v_max.y);

    let res = match algo {
        AlgoType::Grid2dV2 => find_top_std_2(&centers, depth as usize, n_sections as usize, grid_size as usize, rect),
        AlgoType::Grid2dV3 => find_top_std_3(&centers, depth as usize, n_sections as usize, grid_size as usize, rect),
        AlgoType::Grid2dV3a => find_top_std_4(&centers, depth as usize, n_sections as usize, grid_size as usize, rect),
        _ => find_top_std(&centers, depth as usize, grid_size, rect),
    };

    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_and_process_glb() {
        let glb_bytes = include_bytes!("../../test-ht.glb");
        let result = p3d_process(
            glb_bytes,
            InputFileType::Glb,
            AlgoType::Grid2d,
            20,
            10,
            None,
        );
        assert!(result.is_ok(), "Processing test-ht.glb failed: {:?}", result.err());
        if let Ok(output_strings) = result {
            assert!(!output_strings.is_empty(), "Processing test-ht.glb produced no output strings");
        }
    }

    #[test]
    fn test_malformed_glb_data() {
        let malformed_glb_bytes: &[u8] = b"this is not a glb";
        let result = p3d_process(
            malformed_glb_bytes,
            InputFileType::Glb,
            AlgoType::Grid2d,
            20,
            10,
            None,
        );
        assert!(matches!(result, Err(P3DError::GltfError(_))), "Malformed GLB data did not produce GltfError: {:?}", result);
    }

    #[test]
    fn test_gltf_no_geometry() {
        let no_geometry_gltf_json = r#"
        {
          "asset": { "version": "2.0" },
          "scene": 0,
          "scenes": [ { "nodes": [] } ],
          "meshes": []
        }
        "#;
        let no_geometry_gltf_bytes = no_geometry_gltf_json.as_bytes();
        let result = p3d_process(
            no_geometry_gltf_bytes,
            InputFileType::Gltf,
            AlgoType::Grid2d,
            20,
            10,
            None,
        );
        assert!(matches!(result, Err(P3DError::GltfError(_))), "glTF with no geometry did not produce GltfError: {:?}", result);
        if let Err(P3DError::GltfError(msg)) = result {
            assert!(msg.contains("No valid geometry"), "Error message for no geometry is incorrect: {}", msg);
        } else {
            panic!("Expected GltfError for glTF with no geometry, but got Ok or other error: {:?}", result);
        }
    }
}
