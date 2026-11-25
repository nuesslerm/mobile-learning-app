import { cpp } from '@codemirror/lang-cpp';
import { javascript } from '@codemirror/lang-javascript';
import { python } from '@codemirror/lang-python';
import { syntaxTree } from '@codemirror/language';
import { Compartment, RangeSetBuilder, type Extension } from '@codemirror/state';
import { Decoration, ViewPlugin, type EditorView, type ViewUpdate } from '@codemirror/view';
import type { Language } from '@lib/code-editor-api';

/** Holds the editor's language, so switching it reconfigures one compartment and leaves the text, the selection and the undo history alone. */
export const languageCompartment = new Compartment();

export function languageExtension(language: Language): Extension {
  switch (language) {
    case 'python':
      return python();
    case 'javascript':
      return javascript();
    case 'cpp':
      return cpp();
  }
}

const functionBinding = Decoration.mark({ class: 'cm-function-name' });
const typeReference = Decoration.mark({ class: 'cm-type-name' });

type ContrastLanguage = 'javascript' | 'python';

function pythonTypeNames(view: EditorView): Set<string> {
  const names = new Set<string>();
  let annotationEnd = -1;
  syntaxTree(view.state).iterate({
    enter(node) {
      if (node.name === 'TypeDef') annotationEnd = Math.max(annotationEnd, node.to);
      const name = node.name === 'ClassDefinition'
        ? node.node.getChild('VariableName')
        : node.name === 'VariableName' && node.from < annotationEnd ? node : null;
      if (name) names.add(view.state.sliceDoc(name.from, name.to));
    },
  });
  return names;
}

function contrastMarksInView(view: EditorView, language: ContrastLanguage, knownTypes: ReadonlySet<string>) {
  const marks = new RangeSetBuilder<Decoration>();
  const tree = syntaxTree(view.state);
  for (const { from, to } of view.visibleRanges) {
    let annotationEnd = -1;
    tree.iterate({
      from,
      to,
      enter(node) {
        if (language === 'python' && node.name === 'TypeDef') {
          annotationEnd = Math.max(annotationEnd, node.to);
        }
        if (node.from < from || node.from >= to) return;
        if (language === 'python') {
          const inAnnotation = node.from < annotationEnd && (node.name === 'VariableName' || node.name === 'PropertyName');
          const constructor = node.name === 'VariableName'
            && node.node.parent?.name === 'CallExpression'
            && node.node.prevSibling === null
            && knownTypes.has(view.state.sliceDoc(node.from, node.to));
          if (inAnnotation || constructor) {
            marks.add(node.from, node.to, typeReference);
          }
          return;
        }
        if (node.name !== 'VariableDefinition' || node.node.parent?.name !== 'VariableDeclaration') return;
        const equals = node.node.nextSibling;
        if (equals?.name !== 'Equals') return;
        const initializer = equals.nextSibling?.name;
        if (initializer === 'FunctionExpression' || initializer === 'ArrowFunction') {
          marks.add(node.from, node.to, functionBinding);
        }
      },
    });
  }
  return marks.finish();
}

const contrastSyntax = ViewPlugin.fromClass(class {
  decorations;
  private knownTypes: ReadonlySet<string>;

  constructor(view: EditorView, private readonly language: ContrastLanguage) {
    this.knownTypes = language === 'python' ? pythonTypeNames(view) : new Set();
    this.decorations = contrastMarksInView(view, language, this.knownTypes);
  }

  update(update: ViewUpdate) {
    const syntaxChanged = update.docChanged || syntaxTree(update.startState) !== syntaxTree(update.state);
    if (syntaxChanged && this.language === 'python') this.knownTypes = pythonTypeNames(update.view);
    if (syntaxChanged || update.viewportChanged) {
      this.decorations = contrastMarksInView(update.view, this.language, this.knownTypes);
    }
  }
}, { decorations: (plugin) => plugin.decorations });

const contrastExtensions = {
  javascript: contrastSyntax.of('javascript'),
  python: contrastSyntax.of('python'),
};

export function contrastSyntaxFor(language: string): Extension {
  return language === 'javascript' || language === 'python' ? contrastExtensions[language] : [];
}
