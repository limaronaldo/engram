# Spec: RML-928 Document Ingestion (PDF/MD)

## Contexto
Hoje o Engram só ingere memória textual direta e arquivos de instrução (Project Context). Falta suporte a documentos arbitrários (PDF/MD) com chunking e rastreio de origem. Isso limita casos reais (manuais, handbooks, specs longas, relatórios).

## Objetivo
Permitir que um usuário ingira um documento (Markdown ou PDF) e transforme o conteúdo em **memórias chunked**, com metadados de origem, para busca híbrida e recuperação confiável.

## Não‑objetivos (MVP)
- OCR de imagens em PDF
- Parsing de DOCX/HTML
- Indexação multimodal
- Embeddings externos obrigatórios
- Multi‑tenant (cloud)

## Requisitos Funcionais
1. Aceitar ingestão de **Markdown** e **PDF**.
2. Extrair texto e **dividir em chunks** com tamanho configurável.
3. Criar memórias para cada chunk com metadata:
   - `source_file`, `source_path`, `doc_id`, `chunk_index`, `section_path`, `page` (PDF)
4. Suportar **idempotência** (re‑ingest do mesmo arquivo não duplica chunks).
5. Fornecer ferramenta MCP: `memory_ingest_document`.
6. Retornar resumo da ingestão (chunks criados, ignorados, erro de parse).

## Requisitos Não‑Funcionais
- Latência aceitável para docs de até ~10MB.
- Sem crash em PDF malformado (erro retornado, sem corromper estado).
- Default seguro de tamanho máximo (ex: 10MB).

## Sucesso
- Um PDF/MD gera memórias com metadata consistente.
- Busca por termos do documento retorna chunks corretos.
- Re‑ingest do mesmo arquivo é idempotente.

## Riscos
- Parsing PDF inconsistente (biblioteca frágil)
- Chunking ruim → baixa qualidade de busca
- Aumento rápido do tamanho do DB
