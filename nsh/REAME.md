
## Examples

```console
$ nsh -l -vvvv
$ nsh -l localhost:3333 -vvvv -i ~/.nsh/ssi_ed25519-second
$ nsh -vvvv z6MkoAqovY4AbngfzexpgtSSe1C4DiEbDqyhZRbzNdWEARei@localhost \
      echo@z6MksuP9p2w7RdqXowyEq5dtAju9umaPxcXFTV1zqUyxKt79@localhost:3333
```

Tunneling:
```console
$ nsh -l localhost:3333 -vvvv -i ~/.nsh/ssi_ed25519-second
$ nsh --tunnel localhost:7777 z6MksuP9p2w7RdqXowyEq5dtAju9umaPxcXFTV1zqUyxKt79@localhost:3333 -vvvv
$ telnet localhost 777
```
