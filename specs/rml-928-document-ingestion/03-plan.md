# Plan: RML-928 Document Ingestion

## Arquitetura
- Novo módulo: `src/intelligence/document_ingest.rs`
- Parser por formato:
  - **Markdown:** `pulldown-cmark` para extrair headings + texto
  - **PDF:** `pdf_extract` (baseado em `lopdf`) para texto por página
- Chunking: `Chunker` configurável (tamanho + overlap)
- Persistência: criar memórias com tag `document-chunk`

## Parser (detalhes)
- **Markdown:** construir `section_path` a partir de headings (`#`, `##`, `###`).
  - Conteúdo antes do primeiro heading vira seção `"Preamble"`.
- **PDF:** extrair texto por página.
  - `section_path`: `"Page <n>"` quando não houver headings.

## Dependências (propostas)
- `pulldown-cmark` (Markdown)
- `pdf_extract` (PDF)

## Data Flow
1. Recebe `file_path` (MCP)
2. Detecta formato
3. Extrai texto + seções
4. Chunking + hashing
5. Dedup por `doc_id + chunk_hash`
6. Cria memórias

## Metadados sugeridos
```json
{
  "source_file": "handbook.pdf",
  "source_path": "/docs/handbook.pdf",
  "doc_id": "sha256:<file_hash>",
  "chunk_index": 12,
  "section_path": "Security > Key Rotation",
  "page": 3,
  "chunk_hash": "sha256:<chunk>"
}
```

## MCP Tool
- `memory_ingest_document`
  - params:
    - `path` (string, required)
    - `format` (optional): `auto | md | pdf`
    - `chunk_size` (default 1200)
    - `overlap` (default 200)
    - `max_file_size` (default 10MB)
  - result:
    - `document_id`
    - `chunks_created`
    - `chunks_skipped`
    - `chunks_total`
    - `duration_ms`
    - `warnings` (optional)

### MCP Input Schema (draft)
```json
{
  "type": "object",
  "properties": {
    "path": { "type": "string", "description": "Local file path" },
    "format": { "type": "string", "enum": ["auto", "md", "pdf"], "default": "auto" },
    "chunk_size": { "type": "integer", "default": 1200 },
    "overlap": { "type": "integer", "default": 200 },
    "max_file_size": { "type": "integer", "default": 10485760 }
  },
  "required": ["path"]
}
```

## Limites
- `max_file_size`: 10MB (MVP)

## Testes
- Markdown básico
- PDF pequeno (2 páginas)
- Re‑ingest idempotente
- Arquivo acima do limite → erro
