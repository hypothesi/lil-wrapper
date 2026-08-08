module Rewrap.ReferenceOracle

open System
open System.Text.Json
open Rewrap
open Rewrap.Core.Test

type PositionDto() =
    member val line = 0 with get, set
    member val character = 0 with get, set

type SelectionDto() =
    member val anchor = PositionDto() with get, set
    member val active = PositionDto() with get, set

type SettingsDto() =
    member val column = 0 with get, set
    member val tabWidth = 4 with get, set
    member val doubleSentenceSpacing = false with get, set
    member val reformat = false with get, set
    member val wholeComment = true with get, set

type CustomMarkersDto() =
    member val line = "" with get, set
    member val block: string array = [| ""; "" |] with get, set

type RequestDto() =
    member val id = "" with get, set
    member val operation = "rewrap" with get, set
    member val language = "plaintext" with get, set
    member val path = "" with get, set
    member val customMarkers = CustomMarkersDto() with get, set
    member val settings = SettingsDto() with get, set
    member val selections: SelectionDto array = [||] with get, set
    member val lines: string array = [||] with get, set
    member val newText = "" with get, set
    member val position = PositionDto() with get, set
    member val text = "" with get, set
    member val tabWidth = 4 with get, set

type EditDto() =
    member val id = "" with get, set
    member val startLine = 0 with get, set
    member val endLine = -1 with get, set
    member val lines: string array = [||] with get, set
    member val selections: SelectionDto array = [||] with get, set
    member val isEmpty = true with get, set

type ValueDto() =
    member val id = "" with get, set
    member val value: obj = null with get, set

type CorpusCaseDto() =
    member val id = "" with get, set
    member val language = "" with get, set
    member val settings = SettingsDto() with get, set
    member val input: string array = [||] with get, set
    member val expected: string array = [||] with get, set
    member val selections: SelectionDto array = [||] with get, set
    member val only = false with get, set
    member val reformatAlternative = false with get, set

let position (value: PositionDto) : Position =
    { line = value.line; character = value.character }

let selection (value: SelectionDto) : Selection =
    { anchor = position value.anchor; active = position value.active }

let positionDto (value: Position) =
    PositionDto(line = value.line, character = value.character)

let selectionDto (value: Selection) =
    SelectionDto(anchor = positionDto value.anchor, active = positionDto value.active)

let file (request: RequestDto) =
    let block =
        if isNull request.customMarkers.block || request.customMarkers.block.Length < 2 then
            "", ""
        else
            request.customMarkers.block.[0], request.customMarkers.block.[1]

    let markers: CustomMarkers = { line = request.customMarkers.line; block = block }
    let result: File =
        { language = request.language
          path = request.path
          getMarkers = Func<CustomMarkers>(fun () -> markers) }
    result

let settings (request: RequestDto) : Settings =
        { column = request.settings.column
          tabWidth = request.settings.tabWidth
          doubleSentenceSpacing = request.settings.doubleSentenceSpacing
          reformat = request.settings.reformat
          wholeComment = request.settings.wholeComment }

let getLine (request: RequestDto) =
        Func<int, string>(fun index ->
            if index < request.lines.Length then request.lines.[index] else null)

let editDto id (edit: Edit) =
    EditDto(
        id = id,
        startLine = edit.startLine,
        endLine = edit.endLine,
        lines = edit.lines,
        selections = Array.map selectionDto edit.selections,
        isEmpty = edit.isEmpty
    )

let settingsDto (value: Settings) =
    SettingsDto(
        column = value.column,
        tabWidth = value.tabWidth,
        doubleSentenceSpacing = value.doubleSentenceSpacing,
        reformat = value.reformat,
        wholeComment = value.wholeComment
    )

let corpusCase id reformatAlternative (test: Test) =
    CorpusCaseDto(
        id = id,
        language = test.language,
        settings = settingsDto test.settings,
        input = test.input,
        expected = test.expected,
        selections = Array.map selectionDto test.selections,
        only = test.only,
        reformatAlternative = reformatAlternative
    )

let relativeSpecPath (fileName: string) =
    let normalized = fileName.Replace('\\', '/')
    let marker = "/docs/specs/"
    normalized.Substring(normalized.IndexOf(marker, StringComparison.Ordinal) + marker.Length)

let originalCorpus () =
    Native.files
    |> Array.sort
    |> Array.collect (fun fileName ->
        readSamplesInFile fileName
        |> List.mapi (fun index parsed ->
            match parsed with
            | Ok (test, alternative) ->
                let id = $"{relativeSpecPath fileName}#{index + 1}"
                [| yield corpusCase id false test
                   match alternative with
                   | Some reformat -> yield corpusCase $"{id}:reformat" true reformat
                   | None -> () |]
            | Error (_, _, error) -> failwith $"Original corpus parser failed: {error}")
        |> Array.concat)

let run (request: RequestDto) : obj =
    match request.operation with
    | "rewrap" ->
        Core.rewrap
            (file request)
            (settings request)
            (Array.map selection request.selections)
            (getLine request)
        |> editDto request.id
        |> box
    | "autoWrap" ->
        Core.maybeAutoWrap
            (file request)
            (settings request)
            request.newText
            (position request.position)
            (getLine request)
        |> editDto request.id
        |> box
    | "languageName" ->
        ValueDto(id = request.id, value = box (Core.languageNameForFile (file request))) |> box
    | "languages" -> ValueDto(id = request.id, value = box Core.languages) |> box
    | "corpus" -> ValueDto(id = request.id, value = box (originalCorpus ())) |> box
    | "strWidth" ->
        ValueDto(id = request.id, value = box (Core.strWidth request.tabWidth request.text)) |> box
    | "columnScenario" ->
        let rulers = [| 72; 88 |]
        let initial = Core.getWrappingColumn request.path rulers
        let state: DocState =
            { filePath = request.path
              version = 1
              selections = [| { anchor = { line = 0; character = 0 }; active = { line = 0; character = 0 } } |] }
        let beforeSave = Core.maybeChangeWrappingColumn state rulers
        Core.saveDocState state
        let cycled = Core.maybeChangeWrappingColumn state rulers
        Core.saveDocState state
        let moved =
            { state with
                selections = [| { anchor = { line = 0; character = 1 }; active = { line = 0; character = 1 } } |] }
        let afterMove = Core.maybeChangeWrappingColumn moved rulers
        let changedRulers = Core.getWrappingColumn request.path [| 100; 120 |]
        ValueDto(
            id = request.id,
            value = box [| initial; beforeSave; cycled; afterMove; changedRulers |]
        ) |> box
    | operation -> failwith $"Unknown oracle operation: {operation}"

[<EntryPoint>]
let main _ =
    let options = JsonSerializerOptions(PropertyNameCaseInsensitive = true)
    let input = Console.In.ReadToEnd()
    let requests = JsonSerializer.Deserialize<RequestDto array>(input, options)
    let edits = Array.map run requests
    Console.Out.Write(JsonSerializer.Serialize(edits, options))
    0
