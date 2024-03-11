use arrow::array::ArrayData;
use arrow::pyarrow::FromPyArrow;
use pyo3::Python;
use pyo3::{pyfunction, types::PyString, PyAny};

use crate::lava;
use crate::lava::error::LavaError;

#[pyfunction]
pub fn search_lava_bm25(
    py: Python,
    files: Vec<String>,
    query_tokens: Vec<u32>,
    query_weights: Vec<f32>,
    k: usize,
) -> Result<Vec<(u64, u64)>, LavaError> {
    py.allow_threads(|| lava::search_lava_bm25(files, query_tokens, query_weights, k))
}

#[pyfunction]
pub fn search_lava_substring(
    py: Python,
    files: Vec<String>,
    query: String,
    k: usize,
) -> Result<Vec<(u64, u64)>, LavaError> {
    py.allow_threads(|| lava::search_lava_substring(files, query, k))
}

#[pyfunction]
pub fn get_tokenizer_vocab(py: Python, files: Vec<String>) -> Result<Vec<String>, LavaError> {
    py.allow_threads(|| lava::get_tokenizer_vocab(files))
}

#[pyfunction]
pub fn merge_lava_bm25(
    py: Python,
    condensed_lava_file: String,
    lava_files: Vec<String>,
    uid_offsets: Vec<u64>,
) -> Result<(), LavaError> {
    py.allow_threads(|| lava::merge_lava_bm25(&condensed_lava_file, lava_files, uid_offsets))
}

#[pyfunction]
pub fn merge_lava_substring(
    py: Python,
    condensed_lava_file: String,
    lava_files: Vec<String>,
    uid_offsets: Vec<u64>,
) -> Result<(), LavaError> {
    py.allow_threads(|| lava::merge_lava_substring(&condensed_lava_file, lava_files, uid_offsets))
}

#[pyfunction]
pub fn build_lava_bm25(
    py: Python,
    output_file_name: &PyString,
    array: &PyAny,
    uid: &PyAny,
    tokenizer_file: Option<&PyString>,
) -> Result<(), LavaError> {
    let output_file_name = output_file_name.to_string();
    let array = ArrayData::from_pyarrow(array)?;
    let uid = ArrayData::from_pyarrow(uid)?;
    let tokenizer_file = tokenizer_file.map(|x| x.to_string());

    py.allow_threads(|| {
        lava::build_lava_bm25(
            output_file_name,
            array,
            uid,
            tokenizer_file,
            Some(1.2),
            Some(0.75),
        )
    })
}

#[pyfunction]
pub fn build_lava_substring(
    py: Python,
    output_file_name: &PyString,
    array: &PyAny,
    uid: &PyAny,
    tokenizer_file: Option<&PyString>,
    token_skip_factor: Option<u32>,
) -> Result<(), LavaError> {
    let output_file_name = output_file_name.to_string();
    let array = ArrayData::from_pyarrow(array)?;
    let uid = ArrayData::from_pyarrow(uid)?;
    let tokenizer_file = tokenizer_file.map(|x| x.to_string());

    py.allow_threads(|| {
        lava::build_lava_substring(
            output_file_name,
            array,
            uid,
            tokenizer_file,
            token_skip_factor,
        )
    })
}
