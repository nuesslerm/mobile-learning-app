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

function contrastMarksInView(view: EditorView, language: ContrastLanguage) {
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
          if (node.from < annotationEnd && (node.name === 'VariableName' || node.name === 'PropertyName')) {
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

  constructor(view: EditorView, private readonly language: ContrastLanguage) {
    this.decorations = contrastMarksInView(view, language);
  }

  update(update: ViewUpdate) {
    if (update.docChanged || update.viewportChanged || syntaxTree(update.startState) !== syntaxTree(update.state)) {
      this.decorations = contrastMarksInView(update.view, this.language);
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
