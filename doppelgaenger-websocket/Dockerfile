FROM registry.access.redhat.com/ubi8/ubi-minimal:latest

LABEL org.opencontainers.image.source="https://github.com/drogue-iot/drogue-doppelgaenger"

RUN microdnf -y install python3 python3-pip

RUN mkdir /app

COPY . /app

WORKDIR /app

RUN pip3 install . -r requirements.txt

CMD [ "/usr/bin/python3", "/app/main.py" ]
