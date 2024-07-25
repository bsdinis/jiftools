# `jiftools`

A library and collection of tools to parse, write and modify JIF files (Junction Image Format).

## What is the Junction Image Format

![image](https://encrypted-tbn0.gstatic.com/images?q=tbn:ANd9GcSbATHOX0wE_ZyLXKY-EJafCbYyPLtyTkNXmg&s)

If you know you know.

Moreover, you can find the specification for the JIF in [here](SPEC.md).

## Repo Structure

The repo has three main components:
 - [`jif`](jif/README.md): the library that holds the main functionality and modelling for JIF files;
 - [`tracer-format`](tracer-format/README.md): the library to decode memory traces from junction;
 - [`readjif`](readjif/README.md): a tool to read, view and query JIF files
 - [`jiftool`](jiftool/README.md): a tool to change JIF files (by building interval trees, adding ordering segments)
 - [`cmpjif`](cmpjif/README.md): a tool to produce [upset plots](https://en.wikipedia.org/wiki/UpSet_plot) of the private data held by JIFs
 - [`timejif`](timejif/README.md): a tool to produce plots of unique page accesses over time
 - [`tracejif`](tracejif/README.md): a tool to enhance memory traces with VMA information
