mod build_code_lens;
mod resolve_code_lens;

use build_code_lens::build_code_lens;
use code_analysis::{LuaDeclId, LuaMemberId};
use lsp_types::{CodeLens, CodeLensParams};
use resolve_code_lens::resolve_code_lens;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::context::ServerContextSnapshot;

pub async fn on_code_lens_handler(
    context: ServerContextSnapshot,
    params: CodeLensParams,
    _: CancellationToken,
) -> Option<Vec<CodeLens>> {
    let uri = params.text_document.uri;
    let analysis = context.analysis.read().await;
    let file_id = analysis.get_file_id(&uri)?;
    let mut semantic_model = analysis.compilation.get_semantic_model(file_id)?;

    build_code_lens(&mut semantic_model)
}

pub async fn on_resolve_code_lens_handler(
    context: ServerContextSnapshot,
    code_lens: CodeLens,
    _: CancellationToken,
) -> CodeLens {
    let analysis = context.analysis.read().await;
    let compilation = &analysis.compilation;
    let client_id = context
        .config_manager
        .read()
        .await
        .client_config
        .client_id;

    resolve_code_lens(compilation, code_lens.clone(), client_id).unwrap_or(code_lens)
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CodeLensData {
    Member(LuaMemberId),
    DeclId(LuaDeclId),
}
