# Standard fn-style multi-stage build (matches what `fn build` generates for
# a Python function) so this can be built with plain `docker build` too —
# no dependency on the `fn` CLI being installed. See scripts/deploy-budget-guard-function.sh.
FROM fnproject/python:3.11-dev as build-stage
WORKDIR /function
ADD requirements.txt /function/
RUN pip3 install --target /python/ --no-cache --no-cache-dir -r requirements.txt \
    && rm -fr ~/.cache/pip /tmp* requirements.txt \
    && chmod -R o+r /python
ADD . /function/

FROM fnproject/python:3.11
WORKDIR /function
COPY --from=build-stage /python /python
COPY --from=build-stage /function /function
RUN chmod -R o+r /function
ENV PYTHONPATH=/function/:/python/
ENTRYPOINT ["/python/bin/fdk", "/function/func.py", "handler"]
