# QMLDiff - a program for editing QML trees.

## The diff language

### Statements

QMLDiff defines its own language for modifying QML trees. The language's syntax is partially inspired by BASIC, and the JS DOM's querySelector syntax.

In the global scope, you are required to define which file
is going to be alrered. To do that, use the `ALTER` keyword.

Example:
```
ALTER /qml/TestQML.qml
    ; Diff statements go here.
END ALTER
```

Alternatively, you can add data to a `SLOT`. `SLOT`s' contents will be written to the final re-emitted QML in place of `INSERT SLOT <slot>`, or QML `~{slot}~` statements.

Example:
```
SLOT slot
    INSERT {
        // QML Tree goes here.
    }
END SLOT
```
You can only use `INSERT` directives within `SLOT` declarations.

Within `ALTER` statements, you can use the following DIFF directives:

#### `TRAVERSE <tree>`

The traverse statement changes the current root of the file being processed.

Assume the following QML file:
```
import test.Test 1.0

Rectangle {
    Item {
        color: "black"
    }
    Item {
        color: "red"
    }
    Item {
        color: "green"
    }
}
```

To place your cursor within the red `Item`, use the following statment:
```
TRAVERSE Rectangle > Item[.color="\"red\""]
```

`TRAVERSE` blocks can included in one another, to modify objects deeper in the tree structure of
the current root. Because of that, every traverse block needs to be terminated with `END TRAVERSE`

#### `LOAD <file_path>`

The load statements loads the file with the path given as a QMLDiff file.

#### `ASSERT <tree>`

The ASSERT statement disambiguates a TRAVERSE statement by selecting only such roots, that
contain a node matching the given filter. It does not change the root, or move the cursor.

Assume the following QML file:
```
import test.Test 1.0

Rectangle {
    Item {
        color: "black"
    }
    Item {
        color: "red"
        Object {
            OtherObject {
                value: a
            }
        }
    }
    Item {
        color: "red"
        Object {
            OtherObject {
                value: b
            }
        }
    }
}
```

After issuing the instruction `TRAVERSE Rectangle > Item[.color="\"red\""]`, it would be
ambiguous in which item we are currently located.

To narrow down the root to the first Item object, after the `TRAVERSE` statement, you need to issue
the following statement: `ASSERT Object > OtherObject[.value=a]`

#### `LOCATE <BEFORE/AFTER> <tree/ALL>`

The `LOCATE` statement moves the cursor within the current QML tree object to `BEFORE`/`AFTER` the first element matching the `tree`, or all elements.

Assume the following QML file:
```
import test.Test 1.0

Rectangle {
    Item {
        color: "black"
    }
    Item {
        color: "red"
        Object {
            OtherObject {
                value: a
            }
        }
    }
    Item {
        color: "red"
        Object {
            OtherObject {
                value: b
            }
        }
    }
}
```

To move the cursor inbetween the two red items, the following statement would need to be issued (assuming the current root is the Rectangle object):
```
LOCATE AFTER Item > Object > OtherObject[.value=a]
```

#### `INSERT SLOT <slot>`

Inserts a named slot at the current cursor position.

#### `INSERT { QML }`

Inserts the QML code at the current cursor position. It's possible to declare slots within the QML Code by using the `~{slotName}~` syntax:

Assume the following QML file:
```
import test.Test 1.0

Rectangle {
    Item {
        color: "black"
    }
    Item {
        color: "red"
        Object {
            OtherObject {
                value: a
            }
        }
    }
    Item {
        color: "red"
        Object {
            OtherObject {
                value: b
            }
        }
    }
}
```

After executing the following code:

```
TRAVERSE Rectangle
LOCATE AFTER Item > Object > OtherObject[.value=a]
INSERT {
    TestObject {
        function processObject(){
            for(let i = 0; i<10; i++){
                ~{slotLoop}~
            }
        }
    }
}
END TRAVERSE

;-----------In global context-----------;
SLOT slotLoop
    INSERT {
        console.log(i);
    }
END SLOT
```

The QML code would be changed to:

```
import test.Test 1.0

Rectangle {
    Item {
        color: "black"
    }
    Item {
        color: "red"
        Object {
            OtherObject {
                value: a
            }
        }
    }
    TestObject {
        function processObject(){
            for(let i = 0; i<10; i++){
                console.log(i);
            }
        }
    }
    Item {
        color: "red"
        Object {
            OtherObject {
                value: b
            }
        }
    }
}
```


#### `REMOVE <node>`

Deletes all children matching the `<node>` selector from the current root.

#### `REPLACE <node> WITH { QML }`

This statement can be treated as a combination of the `LOCATE`, `REMOVE` and `INSERT` statements.
It locates the first child matching the `<node>` selector within the current root, deletes it, then inserts
the QML code provided at that spot.

#### `RENAME <node> TO <id>`

Renames the first child matching the `<node>` selector to `<id>`. It can only be used for named objects declarations (and not objects!).

Useful for when a function needs to be replaced. This statement makes it possible to simply rename the original function to something else, then insert a new one
named after the original, which invokes its predecessor.

Updates the cursor to after the renamed element.

#### `IMPORT <object> <version> [alias]`

This statement can only be used within the direct scope of the `AFFECT` block (i.e. Not in a `SLOT` or `TRAVERSE` block).
It adds an import to the top of the QML file.


### Selectors

#### Tree Selector

The tree selector consists of multiple node selectors delimeted with the '>' character

#### Node selector

Node selectors can either be simply the name of a given property of a QML object, or a complex selector that checks multiple aspects of a given object.

In the case of the latter, the selector follows the format:

```
ObjectName[property1][property2]...
```

Properties can verify:

- Object name within the parent (`:name`)
- Existence of a given property (`!prop`)
- Equality of a given property (`.prop=value`) *
- Whether or not a given property contains some string (`.prop~value`) *
- The id of a given object (`#root`) (really just syntax sugar for `.id=root`)

\* - The value is checked as-is, but it can be provided as a string. For example, the selectors `Object[.value=test]` or `Object[.value="test"]` won't match the QML object `Object { value: "test" }`. Instead, you need to use `Object[.value="\"test\""]`.


The `[]` characters are ignored within selectors. `Object[.name=test]` is equal to `Object.name=test`.


### Hashing

QMLDiff's diff files can be hashed to not refer to objects, properties or values by their actual names.
Instead, hashes can be used.

Take the following diff file:

```
AFFECT /test.qml
    TRAVERSE RootObject
        LOCATE BEFORE ALL
        INSERT { property bool myValue: false }

        REPLACE visible WITH {
            visible: !global.visible && myValue
        }
    END TRAVERSE
END AFFECT
```

To QMLDiff, it is identical to:

```
AFFECT [[254452526029728816]]
    TRAVERSE [[8398551154981323716]]
        LOCATE BEFORE ALL
        INSERT { property bool myValue: false }

        REPLACE [[233748328658231]] WITH {
            ~&233748328658231&~: !~&7082699062074&~.~&233748328658231&~ && myValue
        }
    END TRAVERSE
END AFFECT
```

In order to retrieve the original names, QMLDiff uses `hashtab` files.
In the case of the aforementioned example, the hashtab consists of the following entries:

```
8398551154981323716 = "RootObject"
254452526029728816 = "/test.qml"
233748328658231 = "visible"
7082699062074 = "global"
```

### Templates

QMLDiff supports diff templating. Templates can be defined, then used in multiple places. They act like macros. Templates are completely separate from slots, and slots cannot be used with them in any way (of course templates can still be inserted into slots).

#### Defining a template

To define a template, use a `TEMPLATE` directive:

```
TEMPLATE SomeTemplate {
    ObjectToBeTemplated {
        someValue: "constant"
        alwaysTrue: true
        name: ~{name}~

        ChildObject {
            childValue: ~{child}~
        }
    }
}
```

Templates work by defining an internal slot scope. That means that anything that would be a slot in normal diff code, would become a property in the template.

#### Inserting a template

Inserting a template can be done using an `INSERT TEMPLATE` directive:

```
INSERT TEMPLATE SomeTemplate {
    child: 'SomeChildValue'
    name: 'Some test object that uses templates'
}
```

#### More complex examples

Templates can also pass whole objects, or objects from slots 

This file:

```
Object {
	someValue: 10
	Something {
		a: 10
	}
	Something {
		b: 10
	}
}
```

Can get translated into this file:

```
Object {
    someValue: 10
    Something {
        a: 10
        test: 100000
        ObjectToBeTemplated {
            someValue: "constant"
            alwaysTrue: true
            name: "Test Object"
            ChildObject {
                ObjectChildA {
                    childa: true
                }

                ObjectChildB {
                    childb: true
                }
            }
        }
    }

    Something {
        b: 10
    }
}
```

Using this template:

```
TEMPLATE SomeTemplate {
    ObjectToBeTemplated {
        someValue: "constant"
        alwaysTrue: true
        name: ~{name}~

        ChildObject {
            ~{children}~
        }
    }
}

AFFECT /main.qml
    TRAVERSE Object > Something[.a=10] 
        LOCATE AFTER ALL INSERT {
            test: 100000
        }
        INSERT TEMPLATE SomeTemplate {
            name: "Test Object"
            children: ObjectChildA {
                childa: true
            }
            children: ObjectChildB {
                childb: true
            }
        }
    END TRAVERSE
END AFFECT
```

## Using QMLDiff as a command-line tool:

QMLDiff can be used as a command-line tool.

Right now the following subcommands are supported:

- create-hashtab `<QML root> [output hashtab path]`
    * Creates a hashtab file from all the files within `QML root` recursively.
- hash-diffs `<hashtab> <diff 1> [diff 2]... [-r]`
    * Turns all the diffs provided into their hashed versions (using the provided hashtab). This operation changes the diffs IN PLACE!
    * `-r` flag reverts this operation.
- apply-diffs `<hashtab> <QML root> <QML destination> [...diffs] [-f] [-c]`
    * Applies all the provided diffs to the QML files within QML root, then writes the results to QML destination.
    * `-f` flattens the output file tree into the root directory
    * `-c` deletes the QML destination directory before applying the diffs.

## Using QMLDiff as a library:

QMLDiff can be used as a C library. It exports the following functions:

- `int qmldiff_build_change_files(const char *rootDir)`
    * Loads all the diff files from rootDir
    * Returns the amount of files read
- `char *qmldiff_process_file(const char *fileName, char *contents, size_t contentsLength)`
    * Processes a single QML file using diffs loaded via `qmldiff_build_change_files`
    * Returns NULL in case of an error, or when no changes were performed. Newly allocated string containing the re-emitted QML otherwise
- `char qmldiff_is_modified(const char *fileName)`
    * Checks if any diff affects the file `fileName`
    * Returns true if they do, false otherwise
- `void qmldiff_start_saving_thread()`
    * Starts the hashtab-exporting thread *
    * Should be called as part of the initialization sequence of your program.
- `void qmldiff_load_rules(const char *rules)`
    * Sets the global hashtab-creation rules to the argument given
    * `rules` are meant to be passed as a raw string containing the hashtab rules. Not a file path!

\* - In order to create a hashtab when QMLDiff is utilized as a library, please set the `QMLDIFF_HASHTAB_CREATE` environment variable to the desired path where the hashtab file is to be kept. This will essentially disable all the diff-applying functionality of QMLDiff. It will be saving the current state of the global hashtab into the desired file every minute, until terminated.


## TODOs:

- [ ] Better error handling - currently both syntax and processing errors are ambiguous
- [ ] Better documentation
- [ ] Better emitters - the current ones make the output QML a bit unreadable
- [ ] Better method of exporting hashtab when running as a library
