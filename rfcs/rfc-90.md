# Ecosystem

* Issue: [#90](https://github.com/henry40408/lmb/issues/90)
* Author: [@henry40408](https://github.com/henry40408)

## What's the ecosystem?

* An "ecosystem" is a combination of scheduled scripts and scripts to handle requests. Take Ruby on Rails as an example, a Rails application may have a main server to handle requests and a scheduler to handle background jobs.
* All scripts in the ecosystem share the same SQLite database.

## How to start an "ecosystem"?

```sh
./target/release/lmb ecosystem --file ecosystem.yaml
```

## How to define an "ecosystem"?

```yaml
version: 1

scheduled:
  options:
    bail: 3 # exit the process if there are 3 errors
  scripts:
    # scripts are evaluated in the order of definition
    - name: foo
      # supports multiple cron expressions in case of limitation of cron syntax
      cron:
        - "0 0 * * *"
        - "0 1 * * *"
      script:
        # file and script are mutually exclusive
        file: foo.lua # relative to the directory of ecosystem.yaml
        script: |
            print("foo")

serve:
  scripts:
    # scripts are evaluated in the order of definition
    - name: bar
      headers:
        - name: X-Foo
          value: bar
        - name: X-Baz
          regex: ^[0-9]+$
      method:
        - GET
        - POST
      path:
        - /bar
        - /baz
      script:
        # file and script are mutually exclusive
        file: bar.lua # relative to the directory of ecosystem.yaml
        script: |
            print("bar")
```
