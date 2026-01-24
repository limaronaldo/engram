# Clarify: RML-928 Document Ingestion

## Perguntas
1. Tamanho máximo do documento no MVP? (sugestão: 10MB)
2. Chunking baseado em caracteres, tokens ou headings?
3. Nome do tool MCP: `memory_ingest_document` ou `memory_ingest`?
4. Ingestão deve suportar diretórios (batch) ou apenas um arquivo por chamada?
5. Onde armazenar o arquivo original (opcional) — metadados apenas ou cópia em disco?

## Decisões tomadas
- **Tamanho máximo:** 10MB no MVP.
- **Chunking:** por tamanho (chars) com overlap; headings só ajudam a montar `section_path`.
- **Tool MCP:** `memory_ingest_document`.
- **Escopo:** apenas **um arquivo por chamada** no MVP (batch fica para depois).
- **Armazenamento do binário:** não armazenar o arquivo original; apenas metadados + hashes.
- **Biblioteca PDF:** `pdf_extract` (baseada em `lopdf`).

## Defaults sugeridos
- `chunk_size`: 1200 chars
- `chunk_overlap`: 200 chars
- `max_file_size`: 10MB
