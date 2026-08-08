import fastDiff from 'npm:fast-diff@1.2.0'
import fs from 'node:fs'
import path from 'node:path'
import {fileURLToPath, pathToFileURL} from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
const source = path.join(root, 'vendor/rewrap/vscode/src')

const trace = {
    autoWrap: [],
    changedColumns: [],
    rewrap: [],
    savedStates: [],
    wrappingColumns: [],
}

let nextEdit = emptyEdit()
let nextColumn

const core = {
    noCustomMarkers: {line: '', block: ['', '']},
    getWrappingColumn(filePath, columns) {
        trace.wrappingColumns.push({filePath, columns})
        return columns[0] ?? 0
    },
    maybeAutoWrap(file, settings, newText, position, getLine) {
        trace.autoWrap.push({
            file: plainFile(file),
            settings,
            newText,
            position,
            firstLine: getLine(0),
        })
        return nextEdit
    },
    maybeChangeWrappingColumn(state, columns) {
        trace.changedColumns.push({state: plainState(state), columns})
        return nextColumn ?? columns[0] ?? 0
    },
    rewrap(file, settings, selections, getLine) {
        trace.rewrap.push({
            file: plainFile(file),
            settings,
            selections: selections.map(plainSelection),
            lines: readLines(getLine),
        })
        return nextEdit
    },
    saveDocState(state) {
        trace.savedStates.push(plainState(state))
    },
}

const commandHandlers = new Map()
const changeListeners = new Set()
const editorListeners = new Set()
const configurationListeners = new Set()
const informationMessages = []
const statusItems = []
const inputValues = []

class Position {
    constructor(line, character) {
        this.line = line
        this.character = character
    }

    translate(lineDelta = 0, characterDelta = 0) {
        return new Position(this.line + lineDelta, this.character + characterDelta)
    }

    isEqual(other) {
        return this.line === other.line && this.character === other.character
    }
}

class Range {
    constructor(startOrLine, startCharacterOrEnd, endLine, endCharacter) {
        if (startOrLine instanceof Position) {
            this.start = startOrLine
            this.end = startCharacterOrEnd
        } else {
            this.start = new Position(startOrLine, startCharacterOrEnd)
            this.end = new Position(endLine, endCharacter)
        }
    }

    isEqual(other) {
        return this.start.isEqual(other.start) && this.end.isEqual(other.end)
    }
}

class Selection extends Range {
    constructor(anchorOrLine, anchorCharacterOrActive, activeLine, activeCharacter) {
        const anchor = anchorOrLine instanceof Position
            ? anchorOrLine
            : new Position(anchorOrLine, anchorCharacterOrActive)
        const active = anchorOrLine instanceof Position
            ? anchorCharacterOrActive
            : new Position(activeLine, activeCharacter)
        super(anchor, active)
        this.anchor = anchor
        this.active = active
    }

    get isEmpty() {
        return this.anchor.isEqual(this.active)
    }
}

class ThemeColor {
    constructor(id) {
        this.id = id
    }
}

class Uri {
    constructor(value) {
        this.value = value
    }

    toString() {
        return this.value
    }
}

class Document {
    constructor({fileName, languageId = 'plaintext', lines, uri, version = 1}) {
        this.fileName = fileName
        this.languageId = languageId
        this.lines = [...lines]
        this.uri = new Uri(uri ?? `file://${fileName}`)
        this.version = version
    }

    get lineCount() {
        return this.lines.length
    }

    lineAt(index) {
        if (index < 0 || index >= this.lines.length) throw new RangeError(`invalid line ${index}`)
        return {text: this.lines[index]}
    }

    validateRange(range) {
        const lastLine = Math.max(this.lines.length - 1, 0)
        const clamp = point => {
            const line = Math.max(0, Math.min(point.line, lastLine))
            const character = Math.max(0, Math.min(point.character, this.lines[line].length))
            return new Position(line, character)
        }
        return new Range(clamp(range.start), clamp(range.end))
    }

    replace(range, text) {
        const replacement = text.split('\n')
        const before = this.lines[range.start.line].slice(0, range.start.character)
        const after = this.lines[range.end.line].slice(range.end.character)
        replacement[0] = before + replacement[0]
        replacement[replacement.length - 1] += after
        this.lines.splice(range.start.line, range.end.line - range.start.line + 1, ...replacement)
        this.version++
    }
}

class Editor {
    constructor(document, selections, tabSize = 4) {
        this.document = document
        this.selections = selections
        this.options = {tabSize}
        this.editResult = true
        this.versionBeforeEdit = null
    }

    get selection() {
        return this.selections[0]
    }

    set selection(value) {
        this.selections = [value]
    }

    edit(callback) {
        if (this.versionBeforeEdit !== null) this.document.version = this.versionBeforeEdit
        let replacement
        const result = callback({replace: (range, text) => { replacement = {range, text}; return true }})
        if (result === false || !this.editResult) return Promise.resolve(false)
        if (this.editResult instanceof Error) return Promise.reject(this.editResult)
        if (replacement) this.document.replace(replacement.range, replacement.text)
        return Promise.resolve(true)
    }
}

const defaults = {
    'editor.rulers': [],
    'editor.wordWrapColumn': 80,
    'rewrap.autoWrap.enabled': false,
    'rewrap.autoWrap.notification': 'icon',
    'rewrap.doubleSentenceSpacing': false,
    'rewrap.reformat': false,
    'rewrap.wholeComment': true,
    'rewrap.wrappingColumn': 0,
}

let currentConfiguration = configuration()

function configuration(values = {}, inspections = {}) {
    return {
        get(name) {
            return Object.hasOwn(values, name) ? values[name] : defaults[name]
        },
        inspect(name) {
            if (inspections[name]) return inspections[name]
            if (Object.hasOwn(values, name)) {
                return {defaultValue: defaults[name], globalValue: values[name]}
            }
            return {defaultValue: defaults[name]}
        },
    }
}

const vscode = {
    Memento: class {},
    Position,
    Range,
    Selection,
    TextDocument: Document,
    TextDocumentChangeEvent: class {},
    TextEditor: Editor,
    ThemeColor,
    WorkspaceConfiguration: class {},
    commands: {
        registerTextEditorCommand(id, handler) {
            commandHandlers.set(id, handler)
            return {dispose() { commandHandlers.delete(id) }}
        },
    },
    extensions: {all: []},
    window: {
        activeTextEditor: null,
        createStatusBarItem() {
            const item = {
                visible: false,
                hide() { this.visible = false },
                show() { this.visible = true },
            }
            statusItems.push(item)
            return item
        },
        onDidChangeActiveTextEditor(listener) {
            editorListeners.add(listener)
            return disposable(editorListeners, listener)
        },
        showInformationMessage(message) {
            informationMessages.push(message)
        },
        async showInputBox() {
            return inputValues.shift()
        },
    },
    workspace: {
        getConfiguration() {
            return currentConfiguration
        },
        onDidChangeConfiguration(listener) {
            configurationListeners.add(listener)
            return disposable(configurationListeners, listener)
        },
        onDidChangeTextDocument(listener) {
            changeListeners.add(listener)
            return disposable(changeListeners, listener)
        },
    },
}

globalThis.require = specifier => {
    if (specifier === 'fast-diff') return fastDiff
    throw new Error(`Unexpected CommonJS dependency: ${specifier}`)
}
globalThis.__rewrapCore = core
globalThis.__vscode = vscode

const settingsModule = await importModule('Settings.ts')
const fixSelections = (await importModule('FixSelections.ts')).default
const getCustomMarkersFactory = (await importModule('CustomLanguage.ts')).default
const common = await importModule('Common.ts')
const autoWrapFactory = (await importModule('AutoWrap.ts')).default
const extension = await importModule('Extension.ts')

const output = {
    autoWrap: await autoWrapScenarios(),
    commands: await commandScenarios(),
    common: await commonScenarios(),
    customLanguages: customLanguageScenarios(),
    manifest: JSON.parse(
        fs.readFileSync(path.join(root, 'vendor/rewrap/vscode/package.json'), 'utf8'),
    ),
    selections: selectionScenarios(),
    settings: settingsScenarios(),
}

Deno.stdout.writeSync(new TextEncoder().encode(JSON.stringify(output)))
Deno.exit(0)

function importModule(file) {
    return import(pathToFileURL(path.join(source, file)).href)
}

function emptyEdit() {
    return {startLine: 0, endLine: -1, lines: [], selections: [], isEmpty: true}
}

function disposable(collection, listener) {
    return {dispose() { collection.delete(listener) }}
}

function plainPosition(position) {
    return {line: position.line, character: position.character}
}

function plainSelection(selection) {
    return {anchor: plainPosition(selection.anchor), active: plainPosition(selection.active)}
}

function plainState(state) {
    return {
        filePath: state.filePath,
        version: state.version,
        selections: state.selections.map(plainSelection),
    }
}

function plainFile(file) {
    return {path: file.path, language: file.language, markers: file.getMarkers()}
}

function readLines(getLine) {
    const lines = []
    for (let index = 0; ; index++) {
        const line = getLine(index)
        if (line === null) return lines
        lines.push(line)
    }
}

function cursor(line, character) {
    return new Selection(line, character, line, character)
}

function makeDocument(id, lines = ['one two three']) {
    return new Document({
        fileName: `/tmp/${id}.txt`,
        languageId: 'plaintext',
        lines,
        uri: `file:///tmp/${id}.txt`,
    })
}

function settingsScenarios() {
    const cases = [
        ['explicit', {'rewrap.wrappingColumn': 72, 'editor.rulers': [80, 100], 'editor.wordWrapColumn': 88}, 4],
        ['rulers', {'rewrap.wrappingColumn': 0, 'editor.rulers': [80, {column: 100}], 'editor.wordWrapColumn': 88}, 2],
        ['numeric-zero', {'editor.rulers': [0, 100], 'editor.wordWrapColumn': 88}, 4],
        ['detailed-zero', {'editor.rulers': [{column: 0}, 100], 'editor.wordWrapColumn': 88}, 4],
        ['invalid-column', {'rewrap.wrappingColumn': -1}, 4],
        ['fractional-column', {'rewrap.wrappingColumn': 80.5}, 4],
        ['large-column', {'rewrap.wrappingColumn': 121}, 4],
        ['booleans', {
            'rewrap.wrappingColumn': 40,
            'rewrap.doubleSentenceSpacing': true,
            'rewrap.reformat': true,
            'rewrap.wholeComment': false,
            'rewrap.autoWrap.enabled': true,
            'rewrap.autoWrap.notification': 'text',
        }, 8],
        ['invalid-tab', {'rewrap.wrappingColumn': 40}, 0],
    ]

    return cases.map(([id, values, tabSize]) => {
        const document = makeDocument(`settings-${id}`)
        const editor = new Editor(document, [cursor(0, 0)], tabSize)
        currentConfiguration = configuration(values)
        const warnings = []
        const originalWarn = console.warn
        console.warn = (...args) => warnings.push(args.map(String))
        try {
            return {
                id,
                input: {values, tabSize},
                editor: settingsModule.getEditorSettings(editor),
                core: settingsModule.getCoreSettings(editor, columns => columns[0]),
                warnings,
            }
        } catch (error) {
            return {id, input: {values, tabSize}, error: error.message, warnings}
        } finally {
            console.warn = originalWarn
        }
    }).concat(scopeScenario())
}

function scopeScenario() {
    const values = {
        'editor.rulers': [90],
        'rewrap.autoWrap.enabled': true,
    }
    const inspections = {
        'editor.rulers': {defaultLanguageValue: [55], workspaceFolderValue: [90]},
        'rewrap.autoWrap.enabled': {defaultLanguageValue: false, workspaceFolderValue: true},
    }
    currentConfiguration = configuration(values, inspections)
    const editor = new Editor(makeDocument('settings-scope'), [cursor(0, 0)])
    const result = settingsModule.getEditorSettings(editor)
    return [{
        id: 'scope-origin',
        input: {values, inspections, tabSize: 4},
        columns: result.columns,
        autoWrap: result.autoWrap,
    }]
}

function selectionScenarios() {
    const cases = [
        {
            id: 'empty-replacement',
            oldLines: ['one'],
            newLines: [],
            startLine: 0,
            endLine: 0,
            selections: [cursor(0, 2)],
        },
        {
            id: 'growth',
            oldLines: ['one two three'],
            newLines: ['one two', 'three'],
            startLine: 0,
            endLine: 0,
            selections: [cursor(0, 8), cursor(1, 2)],
        },
        {
            id: 'contraction',
            oldLines: ['one two', 'three four'],
            newLines: ['one two three four'],
            startLine: 0,
            endLine: 1,
            selections: [new Selection(1, 5, 0, 4), cursor(2, 1)],
        },
        {
            id: 'utf16',
            oldLines: ['a 😀 word'],
            newLines: ['a 😀', 'word'],
            startLine: 0,
            endLine: 0,
            selections: [cursor(0, 7)],
        },
        {
            id: 'repeated',
            oldLines: ['foo bar foo bar'],
            newLines: ['foo bar', 'foo bar'],
            startLine: 0,
            endLine: 0,
            selections: [cursor(0, 12)],
        },
        {
            id: 'before-and-below',
            oldLines: ['middle text'],
            newLines: ['middle', 'text'],
            startLine: 2,
            endLine: 2,
            selections: [cursor(0, 3), new Selection(2, 3, 4, 2)],
        },
        {
            id: 'end-of-line',
            oldLines: ['one two'],
            newLines: ['one', 'two'],
            startLine: 0,
            endLine: 0,
            selections: [cursor(0, 7)],
        },
    ]
    return cases.map(item => ({
        ...item,
        result: fixSelections(item.oldLines, item.selections, {
            startLine: item.startLine,
            endLine: item.endLine,
            lines: item.newLines,
        }).map(plainSelection),
        selectionsInput: item.selections.map(plainSelection),
    })).map(({selectionsInput, oldLines, newLines, startLine, endLine, id, result}) => ({
        id,
        oldLines,
        newLines,
        startLine,
        endLine,
        selectionsInput,
        selections: result,
    }))
}

function customLanguageScenarios() {
    const extensions = [
        {extensionPath: '/one', packageJSON: {contributes: {languages: [
            {id: 'line', configuration: 'line.json'},
            {id: 'duplicate', configuration: 'old.json'},
        ]}}},
        {extensionPath: '/two', packageJSON: {contributes: {languages: [
            {id: 'block', configuration: 'block.json'},
            {id: 'duplicate', configuration: 'new.json'},
            {id: 'invalid', configuration: 'invalid.json'},
            {id: 'parse-error', configuration: 'parse-error.json'},
            {id: 'missing-comments', configuration: 'missing-comments.json'},
            {id: 'nonstring-block', configuration: 'nonstring-block.json'},
            {configuration: 'missing-id.json'},
        ]}}},
        {extensionPath: '/broken', get packageJSON() { throw new Error('broken extension') }},
    ]
    const files = {
        '/one/line.json': `{comments: {lineComment: '//',},}`,
        '/two/block.json': `{comments: {blockComment: ['<#', '#>', 'ignored']}}`,
        '/two/new.json': `{comments: {lineComment: '##'}}`,
        '/two/invalid.json': `{comments: {lineComment: 2, blockComment: ['one']}}`,
        '/two/parse-error.json': `{comments:`,
        '/two/missing-comments.json': `{name: 'none'}`,
        '/two/nonstring-block.json': `{comments: {blockComment: [1, 2]}}`,
    }
    const reads = {}
    const getMarkers = getCustomMarkersFactory(extensions, file => {
        reads[file] = (reads[file] ?? 0) + 1
        return files[file]
    })
    const logs = []
    const methods = ['info', 'warn', 'error']
    const original = Object.fromEntries(methods.map(method => [method, console[method]]))
    for (const method of methods) console[method] = (...args) => logs.push([method, ...args.map(String)])
    try {
        return {
            results: [
                'line', 'block', 'duplicate', 'invalid', 'parse-error',
                'missing-comments', 'nonstring-block', 'unknown', 'line', 'unknown',
            ]
                .map(language => ({language, markers: getMarkers(language)})),
            reads,
            logs,
        }
    } finally {
        Object.assign(console, original)
    }
}

async function commonScenarios() {
    const lineDocument = makeDocument('doc-line', ['first', 'last'])
    const getLine = common.docLine(lineDocument)

    const document = makeDocument('apply', ['before', 'one two three', 'after'])
    const selections = [cursor(1, 8), new Selection(2, 3, 0, 2)]
    const editor = new Editor(document, selections)
    vscode.window.activeTextEditor = editor
    await common.applyEdit(editor, {
        startLine: 1,
        endLine: 1,
        lines: ['one two', 'three'],
        selections: selections.map(plainSelection),
        isEmpty: false,
    })

    const staleDocument = makeDocument('stale', ['one two three'])
    const staleEditor = new Editor(staleDocument, [cursor(0, 4)])
    staleEditor.versionBeforeEdit = 2
    vscode.window.activeTextEditor = staleEditor
    await common.applyEdit(staleEditor, {
        startLine: 0,
        endLine: 0,
        lines: ['changed'],
        selections: [plainSelection(cursor(0, 4))],
        isEmpty: false,
    })

    return {
        docLine: [getLine(0), getLine(1), getLine(2)],
        docType: plainFile(common.docType(lineDocument)),
        applied: {lines: document.lines, selections: editor.selections.map(plainSelection)},
        stale: {lines: staleDocument.lines, version: staleDocument.version},
    }
}

async function autoWrapScenarios() {
    const document = makeDocument('auto', ['one two three '])
    const editor = new Editor(document, [cursor(0, 13)])
    vscode.window.activeTextEditor = editor
    currentConfiguration = configuration({
        'rewrap.autoWrap.enabled': true,
        'rewrap.wrappingColumn': 8,
    })
    const state = memento()
    const autoWrap = autoWrapFactory(state)
    await Promise.resolve()

    const baseChange = {
        document,
        contentChanges: [{text: ' ', range: new Range(0, 13, 0, 13), rangeLength: 0}],
    }
    const cases = []
    const run = (id, change, prepare = () => {}) => {
        prepare()
        const before = trace.autoWrap.length
        for (const listener of changeListeners) listener(change)
        cases.push({id, called: trace.autoWrap.length > before})
    }
    run('eligible', baseChange)
    run('wrong-document', {...baseChange, document: makeDocument('other')})
    run('multiple-selections', baseChange, () => { editor.selections = [cursor(0, 1), cursor(0, 2)] })
    run('nonempty-selection', baseChange, () => { editor.selections = [new Selection(0, 1, 0, 2)] })
    run('multiple-changes', {...baseChange, contentChanges: [...baseChange.contentChanges, ...baseChange.contentChanges]}, () => { editor.selections = [cursor(0, 13)] })
    run('replacement', {...baseChange, contentChanges: [{...baseChange.contentChanges[0], rangeLength: 1}]})
    run('ranged-newline', {...baseChange, contentChanges: [{
        text: '\n', range: new Range(0, 13, 0, 14), rangeLength: 1,
    }]})
    run('multi-change-newline', {...baseChange, contentChanges: [
        {text: '\n', range: new Range(0, 13, 0, 13), rangeLength: 0},
        {text: '  ', range: new Range(1, 0, 1, 0), rangeLength: 0},
    ]})
    run('missing-range-length', {...baseChange, contentChanges: [{text: ' ', range: new Range(0, 13, 0, 13)}]})
    run('negative-range-length', {...baseChange, contentChanges: [{...baseChange.contentChanges[0], rangeLength: -1}]})

    currentConfiguration = configuration({
        'rewrap.autoWrap.enabled': false,
        'rewrap.wrappingColumn': 8,
    })
    await autoWrap.editorToggle(editor)
    const afterOn = Object.fromEntries(state.values)
    await autoWrap.editorToggle(editor)
    const afterOff = Object.fromEntries(state.values)

    const flipState = memento()
    currentConfiguration = configuration({
        'rewrap.autoWrap.enabled': false,
        'rewrap.wrappingColumn': 8,
    })
    const flipAutoWrap = autoWrapFactory(flipState)
    await flipAutoWrap.editorToggle(editor)
    currentConfiguration = configuration({
        'rewrap.autoWrap.enabled': true,
        'rewrap.wrappingColumn': 8,
    })
    await flipAutoWrap.editorToggle(editor)
    const afterConfigurationFlip = Object.fromEntries(flipState.values)
    await flipAutoWrap.editorToggle(editor)
    const afterConfigurationFlipReset = Object.fromEntries(flipState.values)

    return {
        eligibility: cases,
        afterOn,
        afterOff,
        afterConfigurationFlip,
        afterConfigurationFlipReset,
    }
}

async function commandScenarios() {
    commandHandlers.clear()
    trace.rewrap.length = 0
    trace.savedStates.length = 0
    trace.changedColumns.length = 0
    currentConfiguration = configuration({
        'rewrap.wrappingColumn': 0,
        'editor.rulers': [8, 12],
    })
    const document = makeDocument('commands', ['first paragraph', '', 'one two three four'])
    const editor = new Editor(document, [cursor(2, 4)])
    vscode.window.activeTextEditor = editor
    nextEdit = emptyEdit()
    nextColumn = 8
    const context = {workspaceState: memento(), subscriptions: []}
    await extension.activate(context)

    const standardReturn = commandHandlers.get('rewrap.rewrapComment')(editor)
    await Promise.resolve()
    await Promise.resolve()

    inputValues.push('12x')
    await commandHandlers.get('rewrap.rewrapCommentAt')(editor)
    inputValues.push('')
    await commandHandlers.get('rewrap.rewrapCommentAt')(editor)
    const beforeCancel = trace.rewrap.length
    inputValues.push(undefined)
    await commandHandlers.get('rewrap.rewrapCommentAt')(editor)
    const cancelled = trace.rewrap.length === beforeCancel
    const successfulCalls = trace.rewrap.slice(-3)

    const failureDocument = makeDocument('command-failure', ['one two three four'])
    const failureEditor = new Editor(failureDocument, [cursor(0, 4)])
    failureEditor.editResult = new Error('edit failed')
    vscode.window.activeTextEditor = failureEditor
    nextEdit = {
        startLine: 0,
        endLine: 0,
        lines: ['one two', 'three four'],
        selections: [plainSelection(cursor(0, 4))],
        isEmpty: false,
    }
    const originalConsole = {error: console.error, log: console.log}
    console.error = () => {}
    console.log = () => {}
    const savedBeforeFailure = trace.savedStates.length
    commandHandlers.get('rewrap.rewrapComment')(failureEditor)
    await new Promise(resolve => setImmediate(resolve))
    console.error = originalConsole.error
    console.log = originalConsole.log
    const failedEditSavedState = trace.savedStates.length === savedBeforeFailure + 1
    nextEdit = emptyEdit()

    return {
        registered: [...commandHandlers.keys()].sort(),
        subscriptions: context.subscriptions.length,
        standardReturn: standardReturn ?? null,
        rewrapCalls: successfulCalls,
        savedStates: trace.savedStates,
        cancelled,
        failedEditSavedState,
    }
}

function memento() {
    const values = new Map()
    return {
        values,
        get(key) { return values.get(key) },
        async update(key, value) {
            if (value === undefined) values.delete(key)
            else values.set(key, value)
        },
    }
}
